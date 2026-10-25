//! The route editor's glue: the view-model of an [`EditorModel`] and the UI-thread
//! side that drives the Slint editor screen.
//!
//! [`EditorUi`] lives on the Slint thread. The screen's callbacks change the model
//! and redraw the view-model at once; nothing waits for the network. The engine
//! only loads a profile for the editor ([`EditorEvent::Opened`]) and saves it
//! ([`Command::SaveRoute`]), and reports back in a frame that carries an
//! [`EditorEvent`].
//!
//! The pure parts (rows, the save plan, the template of a new route, the
//! suggestions) are the ones unit-tested here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use serde_json::json;
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use throne_api::types::{Outbound, RouteShare};
use tokio::sync::mpsc::UnboundedSender;

use crate::engine::Command;
use crate::route_editor::{
    available_ready_lists, copy_name, ActionView, Bucket, CondKind, EditorModel, RuleView,
    SimpleKind,
};
use crate::{
    ActionChoice, AppWindow, BucketItem, ChipItem, CondChoice, CondItem, EditorState,
    ReadyListItem, RuleItem, ValueItem,
};

/// The name of a route made in the editor, until the user names it.
const NEW_ROUTE_NAME: &str = "Мой маршрут";

/// How long the error of a bucket's last add stays under its card.
const BUCKET_ERROR_SHOWN_FOR: Duration = Duration::from_secs(4);

/// Most rule-set names a value suggests.
const MAX_SUGGESTIONS: usize = 8;

/// The value a flag condition holds when it is on.
const FLAG_ON: &str = "да";

/// What the engine tells the editor. It travels in a frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorEvent {
    /// Open the editor on this profile. `id` is `None` for a new route, and
    /// `rule_sets` is `None` when Throne does not say which rule-sets exist.
    Opened {
        id: Option<i64>,
        share: RouteShare,
        is_remote: bool,
        rule_sets: Option<Vec<String>>,
    },
    /// The profile is saved: the screen goes back to the Routes page.
    Saved,
    /// Saving failed; the text goes to the editor's error line.
    SaveFailed(String),
}

/// The profile a new route starts from: «Мой маршрут» over the proxy, with the two
/// infrastructure rules Bypass Russia has (DNS is hijacked, private addresses go
/// straight out). Simple rules are added after them.
pub fn new_route_share() -> RouteShare {
    serde_json::from_value(json!({
        "kind": "throne-route-profile",
        "v": 1,
        "name": NEW_ROUTE_NAME,
        "default_outbound": "proxy",
        "rules": [
            { "type": "custom", "name": "DNS", "action": "hijack-dns", "protocol": "dns" },
            {
                "type": "custom",
                "name": "Локальная сеть",
                "action": "route",
                "outbound": "direct",
                "ip_is_private": true
            }
        ]
    }))
    .expect("the new route template parses")
}

/// The API call that saves an editor profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavePlan {
    /// `PUT /v1/routes/{id}`: a local profile is replaced.
    Update { id: i64, share: RouteShare },
    /// `POST /v1/routes` with the profile: a new route, or a copy of a remote one.
    Add { share: RouteShare },
}

/// Plans a save. A remote profile is never replaced: `as_copy` saves it as a new
/// local profile named with [`copy_name`]. A new route (`id` is `None`) is added.
pub fn plan_save(id: Option<i64>, mut share: RouteShare, as_copy: bool) -> SavePlan {
    match (id, as_copy) {
        (Some(id), false) => SavePlan::Update { id, share },
        (_, true) => {
            share.name = copy_name(&share.name);
            SavePlan::Add { share }
        }
        (None, false) => SavePlan::Add { share },
    }
}

/// One value chip of a bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipRow {
    /// Position in the bucket, which the screen sends back to remove the value.
    pub index: i32,
    /// The value as the chip shows it: a list without its `list:` prefix.
    pub value: String,
    /// The kind tag, in capitals: САЙТ, IP, ПРОГРАММА…
    pub kind: String,
    /// The color family of the kind: 0 sites, 1 addresses, 2 programs, 3 lists.
    pub tone: i32,
}

/// One of the three buckets of the simple mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketRow {
    pub index: i32,
    pub title: String,
    pub count: i32,
    /// The error of the last add, empty when there is none.
    pub error: String,
    pub chips: Vec<ChipRow>,
}

/// One rule of the advanced mode's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRow {
    pub index: i32,
    /// The number the list shows, from 1.
    pub n: i32,
    pub name: String,
    pub summary: String,
    pub action: String,
    /// `vpn`, `direct`, `block`, `dns` or `other`. The screen picks the colors.
    pub action_kind: &'static str,
    pub enabled: bool,
    pub editable: bool,
    pub selected: bool,
}

/// One condition of the selected rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondRow {
    /// Index in [`CondKind::ALL`], which the screen sends back.
    pub kind: i32,
    pub label: String,
    pub flag: bool,
    /// The state of a flag condition.
    pub on: bool,
    /// Whether the values are rule-set names, which get suggestions.
    pub suggest: bool,
    pub values: Vec<String>,
}

/// A kind of condition the «+ Условие» list offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRow {
    pub kind: i32,
    pub label: String,
}

/// A choice of the «То» row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRow {
    pub kind: &'static str,
    pub label: String,
    pub on: bool,
    pub enabled: bool,
}

/// The "Всё остальное" row: VPN or direct, or the label of another outbound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultRow {
    pub proxy: bool,
    pub direct: bool,
    /// Empty for VPN and direct; the name of another outbound otherwise.
    pub other: String,
}

fn count(n: usize) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// Index of a bucket in [`Bucket::ALL`], which the screen uses.
fn bucket_index(bucket: Bucket) -> i32 {
    Bucket::ALL
        .iter()
        .position(|held| *held == bucket)
        .map_or(0, count)
}

fn bucket_at(index: i32) -> Option<Bucket> {
    usize::try_from(index)
        .ok()
        .and_then(|index| Bucket::ALL.get(index))
        .copied()
}

fn cond_index(kind: CondKind) -> i32 {
    CondKind::ALL
        .iter()
        .position(|held| *held == kind)
        .map_or(0, count)
}

fn cond_kind(index: i32) -> Option<CondKind> {
    usize::try_from(index)
        .ok()
        .and_then(|index| CondKind::ALL.get(index))
        .copied()
}

fn position(index: i32) -> Option<usize> {
    usize::try_from(index).ok()
}

fn tone(kind: SimpleKind) -> i32 {
    match kind {
        SimpleKind::Domain
        | SimpleKind::DomainSuffix
        | SimpleKind::DomainKeyword
        | SimpleKind::DomainRegex => 0,
        SimpleKind::IpCidr => 1,
        SimpleKind::ProcessName => 2,
        SimpleKind::RuleSet => 3,
    }
}

/// The key the screen colors an action by.
pub fn action_kind(action: &ActionView) -> &'static str {
    match action {
        ActionView::Proxy => "vpn",
        ActionView::Direct => "direct",
        ActionView::Block => "block",
        ActionView::Dns => "dns",
        ActionView::Other(_) => "other",
    }
}

/// The action the screen chose by its key. `None` for the keys it does not offer.
fn action_from(kind: &str) -> Option<ActionView> {
    match kind {
        "vpn" => Some(ActionView::Proxy),
        "direct" => Some(ActionView::Direct),
        "block" => Some(ActionView::Block),
        _ => None,
    }
}

/// The buckets of the simple mode, as the screen shows them. `errors` holds the
/// error of each bucket's last add, in [`Bucket::ALL`] order.
pub fn bucket_rows(model: &EditorModel, errors: &[String; 3]) -> Vec<BucketRow> {
    Bucket::ALL
        .into_iter()
        .zip(errors)
        .map(|(bucket, error)| bucket_row(model, bucket, error))
        .collect()
}

fn bucket_row(model: &EditorModel, bucket: Bucket, error: &str) -> BucketRow {
    let items = model.simple_items(bucket);
    let chips = items
        .iter()
        .enumerate()
        .map(|(index, item)| ChipRow {
            index: count(index),
            value: item
                .value
                .strip_prefix("list:")
                .unwrap_or(&item.value)
                .to_owned(),
            kind: item.label.to_uppercase(),
            tone: tone(item.kind),
        })
        .collect();
    BucketRow {
        index: bucket_index(bucket),
        title: bucket.title().to_owned(),
        count: count(items.len()),
        error: error.to_owned(),
        chips,
    }
}

/// The rules of the advanced mode's list, in order.
pub fn rule_rows(model: &EditorModel, selected: Option<usize>) -> Vec<RuleRow> {
    model
        .rules()
        .into_iter()
        .map(|rule| RuleRow {
            index: count(rule.index),
            n: count(rule.index + 1),
            name: rule.name,
            summary: rule.summary,
            action: rule.action.label().to_owned(),
            action_kind: action_kind(&rule.action),
            enabled: rule.enabled,
            editable: rule.editable,
            selected: selected == Some(rule.index),
        })
        .collect()
}

/// The conditions of the rule at `index`, with their values.
pub fn cond_rows(model: &EditorModel, index: usize) -> Vec<CondRow> {
    model
        .conditions(index)
        .into_iter()
        .map(|cond| {
            let flag = cond.kind.is_flag();
            CondRow {
                kind: cond_index(cond.kind),
                label: cond.kind.label().to_owned(),
                flag,
                on: flag && cond.values.iter().any(|value| value == FLAG_ON),
                suggest: cond.kind == CondKind::RuleSet,
                values: if flag { Vec::new() } else { cond.values },
            }
        })
        .collect()
}

/// The kinds the «+ Условие» list offers for the rule at `index`: every kind the
/// rule does not have yet, and none for a rule whose conditions cannot change.
pub fn cond_choices(model: &EditorModel, index: usize, editable: bool) -> Vec<ChoiceRow> {
    if !editable {
        return Vec::new();
    }
    let present: Vec<CondKind> = model
        .conditions(index)
        .iter()
        .map(|cond| cond.kind)
        .collect();
    CondKind::ALL
        .into_iter()
        .filter(|kind| !present.contains(kind))
        .map(|kind| ChoiceRow {
            kind: cond_index(kind),
            label: kind.label().to_owned(),
        })
        .collect()
}

/// The «То» choices of a rule. VPN, direct and block are offered when the rule
/// can change; a DNS or other action the rule has shows as one more choice.
pub fn action_rows(view: &RuleView) -> Vec<ActionRow> {
    let current = action_kind(&view.action);
    let mut rows: Vec<ActionRow> = [ActionView::Proxy, ActionView::Direct, ActionView::Block]
        .iter()
        .map(|action| ActionRow {
            kind: action_kind(action),
            label: action.label().to_owned(),
            on: current == action_kind(action),
            enabled: view.editable,
        })
        .collect();
    match &view.action {
        ActionView::Dns => rows.push(ActionRow {
            kind: "dns",
            label: view.action.label().to_owned(),
            on: true,
            enabled: false,
        }),
        ActionView::Other(name) => rows.push(ActionRow {
            kind: "other",
            label: name.clone(),
            on: true,
            enabled: false,
        }),
        ActionView::Proxy | ActionView::Direct | ActionView::Block => {}
    }
    rows
}

/// The "Всё остальное" row of the profile.
pub fn default_row(model: &EditorModel) -> DefaultRow {
    let other = match model.default_outbound() {
        Outbound::Proxy => None,
        Outbound::Direct => {
            return DefaultRow {
                proxy: false,
                direct: true,
                other: String::new(),
            }
        }
        Outbound::Block => Some("Блок".to_owned()),
        Outbound::WarpBypass => Some("Обход WARP".to_owned()),
        Outbound::Unknown(name) => Some(name.clone()),
    };
    DefaultRow {
        proxy: other.is_none(),
        direct: false,
        other: other.unwrap_or_default(),
    }
}

/// The names of the ready-made lists to offer.
pub fn list_names(known: Option<&[String]>) -> Vec<String> {
    available_ready_lists(known)
        .into_iter()
        .map(|list| list.name.to_owned())
        .collect()
}

/// The rule-set names a typed value suggests: the known ones that contain it and
/// are not taken yet, at most [`MAX_SUGGESTIONS`].
pub fn suggestions(known: &[String], typed: &str, taken: &[String]) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    known
        .iter()
        .filter(|name| name.contains(&typed) && !taken.contains(name))
        .take(MAX_SUGGESTIONS)
        .cloned()
        .collect()
}

/// What the editor holds while it is open.
struct Session {
    id: Option<i64>,
    model: EditorModel,
    selected: Option<usize>,
    /// The last add's error of each bucket, with the number of the timer that clears it.
    bucket_errors: [Option<(u64, String)>; 3],
    /// The error of the last action on the rules or conditions. Empty when none.
    notice: String,
    /// The error of the last save.
    error: String,
    saving: bool,
}

impl Session {
    fn new(id: Option<i64>, model: EditorModel) -> Self {
        Self {
            id,
            model,
            selected: None,
            bucket_errors: [None, None, None],
            notice: String::new(),
            error: String::new(),
            saving: false,
        }
    }

    /// A new route is always saveable; a profile that was read needs a change.
    fn is_dirty(&self) -> bool {
        self.id.is_none() || self.model.is_dirty()
    }
}

/// A model the screen shows as a Slint model. A change rewrites only the rows
/// that differ from the last ones shown.
struct Rows<P, S: 'static> {
    last: Vec<P>,
    model: Rc<VecModel<S>>,
}

impl<P: PartialEq, S: Clone + 'static> Rows<P, S> {
    fn new() -> Self {
        Self {
            last: Vec::new(),
            model: Rc::new(VecModel::default()),
        }
    }

    fn handle(&self) -> ModelRc<S> {
        ModelRc::new(Rc::clone(&self.model))
    }

    fn set(&mut self, rows: Vec<P>, to_item: impl Fn(&P) -> S) {
        if rows == self.last {
            return;
        }
        if rows.len() == self.last.len() {
            for (index, (row, old)) in rows.iter().zip(&self.last).enumerate() {
                if row != old {
                    self.model.set_row_data(index, to_item(row));
                }
            }
        } else {
            self.model
                .set_vec(rows.iter().map(to_item).collect::<Vec<S>>());
        }
        self.last = rows;
    }
}

thread_local! {
    /// The editor of the Slint thread. [`bind`] sets it; [`on_event`] reaches it.
    static EDITOR: RefCell<Option<Rc<EditorUi>>> = const { RefCell::new(None) };
}

/// The editor on the Slint thread. Every method runs there.
struct EditorUi {
    window: slint::Weak<AppWindow>,
    commands: UnboundedSender<Command>,
    session: RefCell<Option<Session>>,
    /// The rule-set names Throne knows, `None` when unknown.
    known: RefCell<Option<Vec<String>>>,
    error_seq: Cell<u64>,
    buckets: RefCell<Rows<BucketRow, BucketItem>>,
    lists: RefCell<Rows<String, ReadyListItem>>,
    rules: RefCell<Rows<RuleRow, RuleItem>>,
    actions: RefCell<Rows<ActionRow, ActionChoice>>,
    conds: RefCell<Rows<CondRow, CondItem>>,
    choices: RefCell<Rows<ChoiceRow, CondChoice>>,
}

/// Connects the editor screen's callbacks to the editor. Called once, on the
/// Slint thread, before the window runs. Save and load go through `commands`.
pub fn bind(window: &AppWindow, commands: UnboundedSender<Command>) {
    let ui = Rc::new(EditorUi {
        window: window.as_weak(),
        commands,
        session: RefCell::new(None),
        known: RefCell::new(None),
        error_seq: Cell::new(0),
        buckets: RefCell::new(Rows::new()),
        lists: RefCell::new(Rows::new()),
        rules: RefCell::new(Rows::new()),
        actions: RefCell::new(Rows::new()),
        conds: RefCell::new(Rows::new()),
        choices: RefCell::new(Rows::new()),
    });
    let state = window.global::<EditorState>();
    state.set_buckets(ui.buckets.borrow().handle());
    state.set_lists(ui.lists.borrow().handle());
    state.set_rules(ui.rules.borrow().handle());
    state.set_cur_actions(ui.actions.borrow().handle());
    state.set_conds(ui.conds.borrow().handle());
    state.set_cond_choices(ui.choices.borrow().handle());

    let u = Rc::clone(&ui);
    state.on_set_name(move |name| u.set_name(&name));
    let u = Rc::clone(&ui);
    state.on_rename_rule(move |index, name| u.rename_rule(index, &name));
    let u = Rc::clone(&ui);
    state.on_set_default(move |kind| u.set_default(&kind));
    let u = Rc::clone(&ui);
    state.on_add_simple(move |bucket, text| u.add_simple(bucket, &text));
    let u = Rc::clone(&ui);
    state.on_remove_chip(move |bucket, chip| u.remove_chip(bucket, chip));
    let u = Rc::clone(&ui);
    state.on_add_ready_list(move |name| u.add_ready_list(&name));
    let u = Rc::clone(&ui);
    state.on_select_rule(move |index| u.select_rule(index));
    let u = Rc::clone(&ui);
    state.on_move_rule(move |up| u.move_rule(up));
    let u = Rc::clone(&ui);
    state.on_toggle_rule(move |index| u.toggle_rule(index));
    let u = Rc::clone(&ui);
    state.on_set_action(move |kind| u.set_action(&kind));
    let u = Rc::clone(&ui);
    state.on_delete_rule(move || u.delete_rule());
    let u = Rc::clone(&ui);
    state.on_add_rule(move || u.add_rule());
    let u = Rc::clone(&ui);
    state.on_add_condition(move |kind| u.add_condition(kind));
    let u = Rc::clone(&ui);
    state.on_remove_condition(move |kind| u.remove_condition(kind));
    let u = Rc::clone(&ui);
    state.on_add_value(move |kind, text| u.add_value(kind, &text));
    let u = Rc::clone(&ui);
    state.on_remove_value(move |kind, text| u.remove_value(kind, &text));
    let u = Rc::clone(&ui);
    state.on_set_flag(move |kind, on| u.set_flag(kind, on));
    let u = Rc::clone(&ui);
    state.on_suggest(move |kind, typed| u.suggest(kind, &typed));
    let u = Rc::clone(&ui);
    state.on_save(move || u.save());

    EDITOR.with(|slot| *slot.borrow_mut() = Some(ui));
}

/// Applies an engine event on the Slint thread. Does nothing before [`bind`].
pub fn on_event(event: EditorEvent) {
    let ui = EDITOR.with(|slot| slot.borrow().clone());
    if let Some(ui) = ui {
        ui.on_event(event);
    }
}

impl EditorUi {
    fn on_event(&self, event: EditorEvent) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match event {
            EditorEvent::Opened {
                id,
                share,
                is_remote,
                rule_sets,
            } => {
                *self.known.borrow_mut() = rule_sets;
                let model = EditorModel::from_share(&share, is_remote);
                let name = model.name().to_owned();
                let mut session = Session::new(id, model);
                // Debug only: GIGA_THRONE_EDITOR_RULE selects a rule of the first editor.
                if let Some(index) = crate::dev_aid::take_start_rule() {
                    session.selected = (index < session.model.rules().len()).then_some(index);
                }
                *self.session.borrow_mut() = Some(session);
                let state = window.global::<EditorState>();
                state.set_route_name(name.as_str().into());
                state.set_is_new(id.is_none());
                state.set_remote(is_remote);
                state.set_advanced(crate::dev_aid::take_start_advanced());
                state.set_leave_pending(-1);
                window.set_page(2);
                self.refresh(true);
            }
            EditorEvent::Saved => {
                *self.session.borrow_mut() = None;
                window.set_page(1);
            }
            EditorEvent::SaveFailed(text) => {
                if let Some(session) = self.session.borrow_mut().as_mut() {
                    session.saving = false;
                    session.error = text;
                }
                self.refresh(false);
            }
        }
    }

    /// Draws the whole editor. `push_name` also sets the name of the selected rule
    /// in its field, which only a new selection may do: the field is being typed in.
    fn refresh(&self, push_name: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let state = window.global::<EditorState>();
        let names = list_names(self.known.borrow().as_deref());
        self.lists.borrow_mut().set(names, |name| ReadyListItem {
            name: name.as_str().into(),
        });
        let session = self.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        let model = &session.model;

        let dirty = session.is_dirty();
        let problems = model.validate();
        state.set_dirty(dirty);
        state.set_can_save(dirty && problems.is_empty() && !session.saving);
        let hint = if dirty {
            problems.first().cloned().unwrap_or_default()
        } else {
            String::new()
        };
        state.set_save_hint(hint.as_str().into());
        state.set_error_text(session.error.as_str().into());
        state.set_notice(session.notice.as_str().into());
        state.set_saving(session.saving);
        let default = default_row(model);
        state.set_default_proxy(default.proxy);
        state.set_default_direct(default.direct);
        state.set_default_other(default.other.as_str().into());
        state.set_selected(session.selected.map_or(-1, count));

        let errors: [String; 3] = std::array::from_fn(|slot| {
            session.bucket_errors[slot]
                .as_ref()
                .map_or_else(String::new, |(_, text)| text.clone())
        });
        self.buckets
            .borrow_mut()
            .set(bucket_rows(model, &errors), bucket_item);
        self.rules
            .borrow_mut()
            .set(rule_rows(model, session.selected), rule_item);

        let views = model.rules();
        match session.selected.and_then(|index| views.get(index)) {
            Some(view) => {
                if push_name {
                    state.set_cur_name(view.name.as_str().into());
                }
                state.set_cur_editable(view.editable);
                self.actions
                    .borrow_mut()
                    .set(action_rows(view), action_item);
                self.conds
                    .borrow_mut()
                    .set(cond_rows(model, view.index), cond_item);
                self.choices
                    .borrow_mut()
                    .set(cond_choices(model, view.index, view.editable), choice_item);
            }
            None => {
                if push_name {
                    state.set_cur_name("".into());
                }
                state.set_cur_editable(false);
                self.actions.borrow_mut().set(Vec::new(), action_item);
                self.conds.borrow_mut().set(Vec::new(), cond_item);
                self.choices.borrow_mut().set(Vec::new(), choice_item);
            }
        }
    }

    /// Runs a change on the open session, shows its error as the notice and redraws.
    fn edit(&self, push_name: bool, change: impl FnOnce(&mut Session) -> Result<(), String>) {
        if let Some(session) = self.session.borrow_mut().as_mut() {
            session.notice = change(session).err().unwrap_or_default();
        }
        self.refresh(push_name);
    }

    fn set_name(&self, name: &str) {
        self.edit(false, |s| {
            if name.trim() != s.model.name() {
                s.model.set_name(name);
            }
            Ok(())
        });
    }

    fn rename_rule(&self, index: i32, name: &str) {
        self.edit(false, |s| {
            let Some(index) = position(index) else {
                return Ok(());
            };
            let current = s.model.rules().get(index).map(|rule| rule.name.clone());
            match current {
                Some(current) if current != name.trim() => s.model.rename_rule(index, name),
                _ => Ok(()),
            }
        });
    }

    fn set_default(&self, kind: &str) {
        let outbound = if kind == "direct" {
            Outbound::Direct
        } else {
            Outbound::Proxy
        };
        self.edit(false, |s| s.model.set_default_outbound(outbound));
    }

    /// Adds a typed value to a bucket. Returns whether it was added, so the screen
    /// clears its field; a refused value stays there, and its error shows under the
    /// card for [`BUCKET_ERROR_SHOWN_FOR`].
    fn add_simple(self: &Rc<Self>, bucket: i32, text: &str) -> bool {
        let (Some(bucket), false) = (bucket_at(bucket), text.trim().is_empty()) else {
            return false;
        };
        let slot = position(bucket_index(bucket)).unwrap_or(0);
        let seq = self.error_seq.get() + 1;
        self.error_seq.set(seq);
        let mut added = false;
        if let Some(session) = self.session.borrow_mut().as_mut() {
            match session.model.add_simple(bucket, text) {
                Ok(()) => {
                    session.bucket_errors[slot] = None;
                    added = true;
                }
                Err(err) => {
                    session.bucket_errors[slot] = Some((seq, err));
                }
            }
        }
        self.refresh(false);
        if !added {
            let ui = Rc::clone(self);
            slint::Timer::single_shot(BUCKET_ERROR_SHOWN_FOR, move || {
                ui.clear_bucket_error(slot, seq);
            });
        }
        added
    }

    fn clear_bucket_error(&self, slot: usize, seq: u64) {
        if let Some(session) = self.session.borrow_mut().as_mut() {
            if session.bucket_errors[slot]
                .as_ref()
                .is_some_and(|(held, _)| *held == seq)
            {
                session.bucket_errors[slot] = None;
            }
        }
        self.refresh(false);
    }

    fn remove_chip(&self, bucket: i32, chip: i32) {
        self.edit(false, |s| {
            let (Some(bucket), Some(chip)) = (bucket_at(bucket), position(chip)) else {
                return Ok(());
            };
            if let Some(item) = s.model.simple_items(bucket).into_iter().nth(chip) {
                s.model.remove_simple(bucket, item.kind, &item.value);
            }
            Ok(())
        });
    }

    fn add_ready_list(&self, name: &str) {
        self.edit(false, |s| s.model.add_ready_list(name));
    }

    fn select_rule(&self, index: i32) {
        self.edit(true, |s| {
            if let Some(index) = position(index) {
                if index < s.model.rules().len() {
                    s.selected = Some(index);
                }
            }
            Ok(())
        });
    }

    fn move_rule(&self, up: bool) {
        self.edit(true, |s| {
            let Some(index) = s.selected else {
                return Ok(());
            };
            s.model.move_rule(index, up)?;
            s.selected = Some(if up { index - 1 } else { index + 1 });
            Ok(())
        });
    }

    fn toggle_rule(&self, index: i32) {
        self.edit(false, |s| {
            let Some(index) = position(index) else {
                return Ok(());
            };
            let enabled = s.model.rules().get(index).map(|rule| rule.enabled);
            match enabled {
                Some(enabled) => s.model.set_enabled(index, !enabled),
                None => Ok(()),
            }
        });
    }

    fn set_action(&self, kind: &str) {
        self.edit(false, |s| {
            let (Some(index), Some(action)) = (s.selected, action_from(kind)) else {
                return Ok(());
            };
            s.model.set_action(index, action)
        });
    }

    fn delete_rule(&self) {
        self.edit(true, |s| {
            let Some(index) = s.selected else {
                return Ok(());
            };
            s.model.delete_rule(index)?;
            let left = s.model.rules().len();
            s.selected = (left > 0).then(|| index.min(left - 1));
            Ok(())
        });
    }

    fn add_rule(&self) {
        self.edit(true, |s| {
            s.selected = Some(s.model.add_rule());
            Ok(())
        });
    }

    fn add_condition(&self, kind: i32) {
        self.edit(false, |s| {
            let (Some(index), Some(kind)) = (s.selected, cond_kind(kind)) else {
                return Ok(());
            };
            s.model.add_condition(index, kind)
        });
    }

    fn remove_condition(&self, kind: i32) {
        self.edit(false, |s| {
            let (Some(index), Some(kind)) = (s.selected, cond_kind(kind)) else {
                return Ok(());
            };
            s.model.remove_condition(index, kind)
        });
    }

    /// Adds a value to a condition of the selected rule. Returns whether it was
    /// added, so the screen clears its field.
    fn add_value(&self, kind: i32, text: &str) -> bool {
        if text.trim().is_empty() {
            return false;
        }
        let mut added = false;
        if let Some(session) = self.session.borrow_mut().as_mut() {
            if let (Some(index), Some(kind)) = (session.selected, cond_kind(kind)) {
                match session.model.add_value(index, kind, text) {
                    Ok(()) => {
                        session.notice.clear();
                        added = true;
                    }
                    Err(err) => session.notice = err,
                }
            }
        }
        self.refresh(false);
        added
    }

    fn remove_value(&self, kind: i32, text: &str) {
        self.edit(false, |s| {
            let (Some(index), Some(kind)) = (s.selected, cond_kind(kind)) else {
                return Ok(());
            };
            s.model.remove_value(index, kind, text)
        });
    }

    fn set_flag(&self, kind: i32, on: bool) {
        self.edit(false, |s| {
            let (Some(index), Some(kind)) = (s.selected, cond_kind(kind)) else {
                return Ok(());
            };
            s.model.set_flag(index, kind, on)
        });
    }

    /// Offers the rule-set names that match what is typed in a rule-set condition.
    /// Only the screen's own state changes, so the session is read without a wait.
    fn suggest(&self, kind: i32, typed: &str) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let taken = match (self.session.try_borrow(), cond_kind(kind)) {
            (Ok(session), Some(CondKind::RuleSet)) => session
                .as_ref()
                .and_then(|s| {
                    let index = s.selected?;
                    s.model
                        .conditions(index)
                        .into_iter()
                        .find(|cond| cond.kind == CondKind::RuleSet)
                        .map(|cond| cond.values)
                })
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let names = self
            .known
            .borrow()
            .as_deref()
            .map(|known| suggestions(known, typed, &taken))
            .unwrap_or_default();
        let state = window.global::<EditorState>();
        state.set_suggest_for(kind);
        let items: Vec<SharedString> = names.into_iter().map(SharedString::from).collect();
        state.set_suggestions(ModelRc::new(VecModel::from(items)));
    }

    /// Sends the profile to the engine when it is dirty and valid. The screen shows
    /// the first problem otherwise, and the engine's answer comes back as an event.
    fn save(&self) {
        let command = self.session.borrow().as_ref().and_then(|s| {
            let ready = !s.saving && s.is_dirty() && s.model.validate().is_empty();
            ready.then(|| Command::SaveRoute {
                id: s.id,
                share: s.model.to_share(),
                as_copy: s.model.is_remote(),
            })
        });
        if let Some(command) = command {
            if let Some(session) = self.session.borrow_mut().as_mut() {
                session.saving = true;
                session.error.clear();
            }
            // The engine is gone only when the window is closing.
            let _ = self.commands.send(command);
        }
        self.refresh(false);
    }
}

fn bucket_item(row: &BucketRow) -> BucketItem {
    let chips: Vec<ChipItem> = row
        .chips
        .iter()
        .map(|chip| ChipItem {
            index: chip.index,
            value: chip.value.as_str().into(),
            kind: chip.kind.as_str().into(),
            tone: chip.tone,
        })
        .collect();
    BucketItem {
        index: row.index,
        title: row.title.as_str().into(),
        count: row.count,
        error: row.error.as_str().into(),
        items: ModelRc::new(VecModel::from(chips)),
    }
}

fn rule_item(row: &RuleRow) -> RuleItem {
    RuleItem {
        index: row.index,
        n: row.n,
        name: row.name.as_str().into(),
        summary: row.summary.as_str().into(),
        action: row.action.as_str().into(),
        action_kind: row.action_kind.into(),
        enabled: row.enabled,
        editable: row.editable,
        selected: row.selected,
    }
}

fn cond_item(row: &CondRow) -> CondItem {
    let values: Vec<ValueItem> = row
        .values
        .iter()
        .map(|value| ValueItem {
            text: value.as_str().into(),
        })
        .collect();
    CondItem {
        kind: row.kind,
        label: row.label.as_str().into(),
        flag: row.flag,
        on: row.on,
        suggest: row.suggest,
        values: ModelRc::new(VecModel::from(values)),
    }
}

fn choice_item(row: &ChoiceRow) -> CondChoice {
    CondChoice {
        kind: row.kind,
        label: row.label.as_str().into(),
    }
}

fn action_item(row: &ActionRow) -> ActionChoice {
    ActionChoice {
        kind: row.kind.into(),
        label: row.label.as_str().into(),
        on: row.on,
        enabled: row.enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_of(json: &str) -> EditorModel {
        let share: RouteShare = serde_json::from_str(json).expect("the test profile parses");
        EditorModel::from_share(&share, false)
    }

    const SIMPLE_EMPTY: &str = r#"{"name": "T", "rules": [
        {"name": "Своё", "type": "custom", "action": "route", "outbound": "direct", "domain": ["b.com"]}
    ]}"#;

    fn errors() -> [String; 3] {
        Default::default()
    }

    /// The new-route template with one value in each bucket.
    fn filled() -> EditorModel {
        let mut model = EditorModel::from_share(&new_route_share(), false);
        model.add_simple(Bucket::Proxy, "youtube.com").unwrap();
        model.add_simple(Bucket::Direct, "steam").unwrap();
        model
    }

    #[test]
    fn new_route_starts_with_the_two_infrastructure_rules() {
        let share = new_route_share();
        assert_eq!(share.name, "Мой маршрут");
        assert_eq!(share.default_outbound, Outbound::Proxy);
        assert_eq!(share.rules.len(), 2);
        assert_eq!(share.rules[0].action, "hijack-dns");
        assert_eq!(share.rules[0].protocol.as_deref(), Some("dns"));
        assert_eq!(share.rules[1].action, "route");
        assert_eq!(share.rules[1].outbound, Some(Outbound::Direct));
        assert_eq!(share.rules[1].ip_is_private, Some(true));

        let model = EditorModel::from_share(&share, false);
        assert!(model.validate().is_empty(), "{:?}", model.validate());
        assert!(!model.is_dirty());
    }

    #[test]
    fn simple_values_go_after_the_infrastructure_rules() {
        let model = filled();
        let rules = model.to_share().rules;
        assert_eq!(rules[2].r#type, "simple_process_name_bypass");
        assert_eq!(rules[2].process_name, Some(vec!["steam".to_owned()]));
        assert_eq!(rules[3].r#type, "simple_address_proxy");
        assert_eq!(rules[3].domain_suffix, Some(vec!["youtube.com".to_owned()]));
    }

    #[test]
    fn plan_save_uses_update_for_a_local_profile_and_add_otherwise() {
        let share = new_route_share();
        assert_eq!(
            plan_save(Some(7), share.clone(), false),
            SavePlan::Update {
                id: 7,
                share: share.clone()
            }
        );
        assert_eq!(
            plan_save(None, share.clone(), false),
            SavePlan::Add {
                share: share.clone()
            }
        );
        match plan_save(Some(7), share.clone(), true) {
            SavePlan::Add { share: copy } => {
                assert_eq!(copy.name, "Мой маршрут (копия)");
                assert_eq!(copy.rules, share.rules);
            }
            other => panic!("a copy must be added, got {other:?}"),
        }
    }

    #[test]
    fn bucket_rows_show_kinds_counts_and_errors() {
        let model = filled();
        let mut errs = errors();
        errs[2] = "Не похоже на сайт, IP или программу".to_owned();
        let rows = bucket_rows(&model, &errs);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].title, "Через VPN");
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].count, 1);
        assert_eq!(rows[0].chips[0].value, "youtube.com");
        assert_eq!(rows[0].chips[0].kind, "САЙТ");
        assert_eq!(rows[0].chips[0].tone, 0);
        assert_eq!(rows[1].title, "Напрямую");
        assert_eq!(rows[1].chips[0].value, "steam");
        assert_eq!(rows[1].chips[0].kind, "ПРОГРАММА");
        assert_eq!(rows[1].chips[0].tone, 2);
        assert!(rows[2].chips.is_empty());
        assert_eq!(rows[2].error, "Не похоже на сайт, IP или программу");
        assert!(rows[0].error.is_empty());
    }

    #[test]
    fn ready_list_chips_show_the_list_name() {
        let mut model = EditorModel::from_share(&new_route_share(), false);
        model.add_ready_list("Реклама").unwrap();
        let rows = bucket_rows(&model, &errors());
        assert_eq!(rows[2].chips.len(), 1);
        assert_eq!(rows[2].chips[0].value, "Реклама");
        assert_eq!(rows[2].chips[0].kind, "СПИСОК");
        assert_eq!(rows[2].chips[0].tone, 3);
    }

    #[test]
    fn rule_rows_number_from_one_and_mark_the_selection() {
        let model = filled();
        let rows = rule_rows(&model, Some(3));
        assert_eq!(rows[0].n, 1);
        assert_eq!(rows[0].action_kind, "dns");
        assert_eq!(rows[0].action, "DNS");
        assert_eq!(rows[1].action_kind, "direct");
        assert_eq!(rows[1].summary, "частный IP");
        assert!(rows.iter().all(|row| row.enabled && row.editable));
        assert_eq!(
            rows.iter().map(|row| row.selected).collect::<Vec<_>>(),
            [false, false, false, true]
        );
    }

    #[test]
    fn read_only_rules_are_marked_and_not_editable() {
        let model = model_of(
            r#"{"name": "T", "rules": [
                {"type": "endpoint_preferred_by", "name": "ep", "action": "route", "outbound": "warp-bypass", "endpoint": "wg0"}
            ]}"#,
        );
        let rows = rule_rows(&model, None);
        assert!(!rows[0].editable);
        assert_eq!(rows[0].action_kind, "other");
        assert_eq!(rows[0].action, "warp-bypass");
        assert!(cond_choices(&model, 0, false).is_empty());
        assert!(action_rows(&model.rules()[0])
            .iter()
            .all(|row| !row.enabled));
    }

    #[test]
    fn conditions_show_flags_as_switches() {
        let model = EditorModel::from_share(&new_route_share(), false);
        let rows = cond_rows(&model, 1);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Частные IP (локальная сеть)");
        assert!(rows[0].flag && rows[0].on && rows[0].values.is_empty());
        assert!(!rows[0].suggest);

        let choices = cond_choices(&model, 1, true);
        assert_eq!(choices.len(), CondKind::ALL.len() - 1);
        assert!(choices
            .iter()
            .all(|choice| choice.label != "Частные IP (локальная сеть)"));
        assert!(choices.iter().any(|choice| choice.label == "Сайт (точно)"));
    }

    #[test]
    fn rule_set_conditions_ask_for_suggestions() {
        let model = model_of(
            r#"{"name": "T", "rules": [
                {"name": "Списки", "type": "custom", "action": "route", "outbound": "direct", "rule_set": ["geosite-ru"]}
            ]}"#,
        );
        let rows = cond_rows(&model, 0);
        assert_eq!(rows[0].label, "Список (rule-set)");
        assert!(rows[0].suggest);
        assert_eq!(rows[0].values, ["geosite-ru"]);
    }

    #[test]
    fn action_rows_offer_the_three_and_keep_dns_and_other_visible() {
        let model = EditorModel::from_share(&new_route_share(), false);
        let rows = action_rows(&model.rules()[0]);
        assert_eq!(
            rows.iter().map(|row| row.kind).collect::<Vec<_>>(),
            ["vpn", "direct", "block", "dns"]
        );
        assert!(rows[3].on && !rows[3].enabled);
        assert!(!rows[0].on && rows[0].enabled);

        let rows = action_rows(&model.rules()[1]);
        assert_eq!(rows.len(), 3);
        assert!(rows[1].on);
    }

    #[test]
    fn default_row_names_the_other_outbounds() {
        let mut model = EditorModel::from_share(&new_route_share(), false);
        assert_eq!(
            default_row(&model),
            DefaultRow {
                proxy: true,
                direct: false,
                other: String::new()
            }
        );
        model.set_default_outbound(Outbound::Direct).unwrap();
        assert!(default_row(&model).direct);
        let block: RouteShare =
            serde_json::from_str(r#"{"name": "T", "default_outbound": "block", "rules": []}"#)
                .unwrap();
        let row = default_row(&EditorModel::from_share(&block, false));
        assert_eq!(
            (row.proxy, row.direct, row.other.as_str()),
            (false, false, "Блок")
        );
    }

    #[test]
    fn ready_lists_follow_the_known_rule_sets() {
        assert_eq!(list_names(None).len(), 4);
        let known = vec![
            "geosite-ru".to_owned(),
            "geoip-ru".to_owned(),
            "geosite-category-ru".to_owned(),
        ];
        assert_eq!(list_names(Some(known.as_slice())), ["Российские сайты"]);
        assert!(list_names(Some(&known[..0])).is_empty());
    }

    #[test]
    fn suggestions_match_unused_names_up_to_eight() {
        let known: Vec<String> = (0..12)
            .map(|i| format!("geosite-cat{i}"))
            .chain(["geoip-ru".to_owned()])
            .collect();
        let found = suggestions(&known, " GEOSITE ", &["geosite-cat0".to_owned()]);
        assert_eq!(found.len(), MAX_SUGGESTIONS);
        assert_eq!(found[0], "geosite-cat1");
        assert!(suggestions(&known, "  ", &[]).is_empty());
        assert_eq!(suggestions(&known, "geoip", &[]), ["geoip-ru"]);
    }

    #[test]
    fn a_local_profile_keeps_its_edits_for_the_plan() {
        let mut model = model_of(SIMPLE_EMPTY);
        model.set_name("Дом");
        let plan = plan_save(Some(3), model.to_share(), model.is_remote());
        match plan {
            SavePlan::Update { id, share } => {
                assert_eq!(id, 3);
                assert_eq!(share.name, "Дом");
            }
            other => panic!("expected an update, got {other:?}"),
        }
    }
}
