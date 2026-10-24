//! The advanced mode's rule list: every rule in order as the screen shows it,
//! and the changes to the list itself (order, switch, action, name, adding,
//! deleting).

use throne_api::types::{Outbound, RouteRule};

use super::advanced::summary;
use super::simple::{default_name, parse_type, type_token, Bucket};
use super::EditorModel;

/// What a rule does with the traffic it matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionView {
    /// Through the running profile (`route` to `proxy`).
    Proxy,
    /// Straight out of this computer (`route` to `direct`).
    Direct,
    /// Dropped (`reject`).
    Block,
    /// DNS is hijacked (`hijack-dns`).
    Dns,
    /// Anything else, named as Throne names it. Shown, not chosen here.
    Other(String),
}

impl ActionView {
    /// The short label of a rule's action on the screen.
    pub fn label(&self) -> &str {
        match self {
            ActionView::Proxy => "VPN",
            ActionView::Direct => "Напрямую",
            ActionView::Block => "Блок",
            ActionView::Dns => "DNS",
            ActionView::Other(name) => name,
        }
    }
}

/// Where a rule comes from: the simple mode, or the user's own rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Simple,
    Custom,
}

/// One rule as the advanced mode's list shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleView {
    /// Position in the list, from 0.
    pub index: usize,
    pub name: String,
    /// One line of conditions, in Russian. See [`EditorModel::conditions`] for the values.
    pub summary: String,
    pub action: ActionView,
    pub enabled: bool,
    /// Whether the conditions can be changed here. Other rules are read-only,
    /// but they can still be moved, switched and renamed.
    pub editable: bool,
    pub origin: Origin,
}

impl EditorModel {
    /// Every rule in order.
    pub fn rules(&self) -> Vec<RuleView> {
        self.share
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| RuleView {
                index,
                name: name_of(rule),
                summary: summary(rule),
                action: action_of(rule),
                enabled: !rule.disabled,
                editable: is_editable(rule),
                origin: if rule.r#type.starts_with("simple_") {
                    Origin::Simple
                } else {
                    Origin::Custom
                },
            })
            .collect()
    }

    /// Moves a rule one place up or down. Fails at the ends of the list.
    pub fn move_rule(&mut self, index: usize, up: bool) -> Result<(), String> {
        self.check_rule(index)?;
        let target = if up {
            index
                .checked_sub(1)
                .ok_or_else(|| "Правило уже первое".to_owned())?
        } else {
            let next = index + 1;
            if next >= self.share.rules.len() {
                return Err("Правило уже последнее".to_owned());
            }
            next
        };
        self.share.rules.swap(index, target);
        Ok(())
    }

    /// Switches a rule on or off. A disabled rule is kept and does not match.
    pub fn set_enabled(&mut self, index: usize, enabled: bool) -> Result<(), String> {
        self.rule_mut(index)?.disabled = !enabled;
        Ok(())
    }

    /// Changes what a rule does. A simple rule moves to the bucket's type; a
    /// custom rule gets the outbound, or `hijack-dns` for [`ActionView::Dns`].
    pub fn set_action(&mut self, index: usize, action: ActionView) -> Result<(), String> {
        let rule = self.rule_mut(index)?;
        if let Some((family, _)) = parse_type(&rule.r#type) {
            let bucket = bucket_of(&action).ok_or_else(|| {
                "Простое правило можно направить только через VPN, напрямую или блокировать"
                    .to_owned()
            })?;
            rule.r#type = type_token(family, bucket).to_owned();
            rule.action = bucket.action().to_owned();
            rule.outbound = bucket.outbound();
        } else if rule.r#type == "custom" {
            if action == ActionView::Dns {
                rule.action = "hijack-dns".to_owned();
                rule.outbound = None;
            } else {
                let bucket = bucket_of(&action)
                    .ok_or_else(|| "Такое действие здесь не выбрать".to_owned())?;
                rule.action = bucket.action().to_owned();
                rule.outbound = bucket.outbound();
            }
        } else {
            return Err("Действие этого правила здесь не меняется".to_owned());
        }
        Ok(())
    }

    /// Renames a rule. The name is trimmed; an empty custom name fails validation.
    pub fn rename_rule(&mut self, index: usize, name: &str) -> Result<(), String> {
        self.rule_mut(index)?.name = name.trim().to_owned();
        Ok(())
    }

    /// Deletes a rule.
    pub fn delete_rule(&mut self, index: usize) -> Result<(), String> {
        self.check_rule(index)?;
        self.share.rules.remove(index);
        Ok(())
    }

    /// Appends a new custom rule with no conditions that sends to the VPN, and
    /// returns its index.
    pub fn add_rule(&mut self) -> usize {
        self.share.rules.push(new_rule(
            "Новое правило",
            "custom",
            "route",
            Some(Outbound::Proxy),
        ));
        self.share.rules.len() - 1
    }
}

/// Whether a rule's conditions can be changed here: custom rules and the simple
/// ones. Process-path, endpoint and other types are read-only.
pub(super) fn is_editable(rule: &RouteRule) -> bool {
    rule.r#type == "custom" || parse_type(&rule.r#type).is_some()
}

/// A rule with the given name, type, action and outbound, and every other field
/// at its default. Built through serde so the defaults are the ones the API reads.
/// The outbound is left out when `None`, as Throne writes block rules.
pub(super) fn new_rule(
    name: &str,
    rule_type: &str,
    action: &str,
    outbound: Option<Outbound>,
) -> RouteRule {
    let mut value = serde_json::json!({
        "name": name,
        "type": rule_type,
        "action": action,
    });
    if let Some(outbound) = outbound {
        value["outbound"] = String::from(outbound).into();
    }
    serde_json::from_value(value)
        .expect("a rule with only a name, a type, an action and an outbound deserializes")
}

/// The name a rule shows. A simple rule with no name shows its bucket.
fn name_of(rule: &RouteRule) -> String {
    if !rule.name.trim().is_empty() {
        return rule.name.clone();
    }
    if rule.r#type == "custom" {
        return "Без названия".to_owned();
    }
    match parse_type(&rule.r#type) {
        Some((family, bucket)) => default_name(family, bucket),
        None => rule.r#type.clone(),
    }
}

/// What a rule does. Throne's block rules are `reject`; an older `route` to the
/// block outbound reads the same. Nothing is written until the user changes it.
fn action_of(rule: &RouteRule) -> ActionView {
    match rule.action.as_str() {
        "reject" => ActionView::Block,
        "route" => match &rule.outbound {
            Some(Outbound::Proxy) => ActionView::Proxy,
            Some(Outbound::Direct) => ActionView::Direct,
            Some(Outbound::Block) => ActionView::Block,
            Some(other) => ActionView::Other(String::from(other.clone())),
            // A simple rule without an outbound says it in its type.
            None => match parse_type(&rule.r#type) {
                Some((_, bucket)) => action_of_bucket(bucket),
                None => ActionView::Other("route".to_owned()),
            },
        },
        "hijack-dns" => ActionView::Dns,
        other => ActionView::Other(other.to_owned()),
    }
}

fn action_of_bucket(bucket: Bucket) -> ActionView {
    match bucket {
        Bucket::Proxy => ActionView::Proxy,
        Bucket::Direct => ActionView::Direct,
        Bucket::Block => ActionView::Block,
    }
}

/// The bucket an action maps to. `None` for DNS and the other actions.
fn bucket_of(action: &ActionView) -> Option<Bucket> {
    match action {
        ActionView::Proxy => Some(Bucket::Proxy),
        ActionView::Direct => Some(Bucket::Direct),
        ActionView::Block => Some(Bucket::Block),
        ActionView::Dns | ActionView::Other(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{bypass_russia, model_of, synthetic};
    use super::super::EditorModel;
    use super::*;

    const TWO: &str = r#"{"name": "T", "rules": [
        {"name": "a", "type": "custom", "action": "route", "outbound": "proxy", "domain": ["a.com"]},
        {"name": "b", "type": "custom", "action": "route", "outbound": "direct", "domain": ["b.com"]}
    ]}"#;

    fn names(model: &EditorModel) -> Vec<String> {
        model.rules().into_iter().map(|rule| rule.name).collect()
    }

    #[test]
    fn views_of_bypass_russia() {
        let model = EditorModel::from_share(&bypass_russia(), false);
        let rules = model.rules();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].action, ActionView::Dns);
        assert_eq!(rules[0].summary, "протокол: dns");
        assert_eq!(rules[1].action, ActionView::Direct);
        assert_eq!(rules[1].summary, "частный IP");
        assert_eq!(
            rules[2].summary,
            "список: geoip-ru, geosite-ru, geosite-category-ru, geosite-ru-available-only-inside"
        );
        assert!(rules
            .iter()
            .all(|rule| rule.enabled && rule.editable && rule.origin == Origin::Custom));
    }

    #[test]
    fn views_of_the_synthetic_profile() {
        let model = EditorModel::from_share(&synthetic(), false);
        let rules = model.rules();
        assert_eq!(rules[0].action, ActionView::Dns);
        assert_eq!(rules[1].origin, Origin::Simple);
        assert_eq!(rules[1].name, "Сайты: Блокировать");
        assert_eq!(rules[1].action, ActionView::Block);
        assert!(!rules[2].enabled);
        assert_eq!(rules[2].name, "Программы: Напрямую");
        assert_eq!(rules[2].action, ActionView::Direct);
        assert_eq!(rules[3].action, ActionView::Proxy);
        assert!(!rules[4].editable);
        assert_eq!(rules[4].origin, Origin::Simple);
        assert!(!rules[5].editable);
        assert_eq!(rules[5].origin, Origin::Custom);
        assert_eq!(rules[5].action, ActionView::Other("warp-bypass".to_owned()));
        assert_eq!(rules[6].action, ActionView::Block);
        assert!(rules[6].editable);
        assert_eq!(rules[6].action.label(), "Блок");
    }

    #[test]
    fn simple_rule_without_outbound_reads_its_bucket() {
        let model = model_of(
            r#"{"name": "T", "rules": [{"type": "simple_address_block", "action": "route", "domain": ["x.com"]}]}"#,
        );
        assert_eq!(model.rules()[0].action, ActionView::Block);
    }

    #[test]
    fn move_rule_swaps_and_refuses_the_ends() {
        let mut model = model_of(TWO);
        assert!(model.move_rule(0, true).is_err());
        assert!(model.move_rule(1, false).is_err());
        model.move_rule(1, true).unwrap();
        assert_eq!(names(&model), ["b", "a"]);
        model.move_rule(0, false).unwrap();
        assert_eq!(names(&model), ["a", "b"]);
        assert!(model.move_rule(5, true).is_err());
    }

    #[test]
    fn set_enabled_is_the_disabled_flag() {
        let mut model = model_of(TWO);
        model.set_enabled(0, false).unwrap();
        assert!(model.to_share().rules[0].disabled);
        assert!(!model.rules()[0].enabled);
        model.set_enabled(0, true).unwrap();
        assert_eq!(model.to_share(), model_of(TWO).to_share());
        assert!(model.set_enabled(9, false).is_err());
    }

    #[test]
    fn rename_trims_and_checks_the_index() {
        let mut model = model_of(TWO);
        model.rename_rule(1, "  Новое имя ").unwrap();
        assert_eq!(names(&model), ["a", "Новое имя"]);
        assert!(model.rename_rule(7, "x").is_err());
    }

    #[test]
    fn delete_rule_removes_it() {
        let mut model = model_of(TWO);
        model.delete_rule(0).unwrap();
        assert_eq!(names(&model), ["b"]);
        assert!(model.delete_rule(1).is_err());
    }

    #[test]
    fn add_rule_appends_an_empty_custom_rule() {
        let mut model = model_of(TWO);
        let index = model.add_rule();
        assert_eq!(index, 2);
        let view = &model.rules()[index];
        assert_eq!(view.name, "Новое правило");
        assert_eq!(view.action, ActionView::Proxy);
        assert_eq!(view.summary, "нет условий");
        assert_eq!(view.origin, Origin::Custom);
        let rule = &model.to_share().rules[index];
        assert_eq!(rule.r#type, "custom");
        assert_eq!(rule.action, "route");
    }

    #[test]
    fn custom_rule_actions() {
        let mut model = model_of(TWO);
        model.set_action(0, ActionView::Dns).unwrap();
        assert_eq!(model.to_share().rules[0].action, "hijack-dns");
        assert_eq!(model.to_share().rules[0].outbound, None);
        assert_eq!(model.rules()[0].action, ActionView::Dns);
        model.set_action(0, ActionView::Direct).unwrap();
        assert_eq!(model.to_share().rules[0].action, "route");
        assert_eq!(model.to_share().rules[0].outbound, Some(Outbound::Direct));
        assert!(model
            .set_action(0, ActionView::Other("x".to_owned()))
            .is_err());
    }

    #[test]
    fn simple_rule_action_changes_its_type() {
        let mut model = EditorModel::from_share(&synthetic(), false);
        model.set_action(1, ActionView::Proxy).unwrap();
        let rule = &model.to_share().rules[1];
        assert_eq!(rule.r#type, "simple_address_proxy");
        assert_eq!(rule.outbound, Some(Outbound::Proxy));
        assert_eq!(rule.domain_suffix, Some(vec!["ads.example.com".to_owned()]));
        assert!(model.set_action(1, ActionView::Dns).is_err());
        assert_eq!(model.rules()[1].origin, Origin::Simple);
    }

    #[test]
    fn opaque_rules_keep_their_action() {
        let mut model = EditorModel::from_share(&synthetic(), false);
        assert!(model.set_action(4, ActionView::Direct).is_err());
        assert!(model.set_action(5, ActionView::Direct).is_err());
        assert_eq!(
            model.to_share(),
            EditorModel::from_share(&synthetic(), false).to_share()
        );
    }

    #[test]
    fn block_is_read_from_reject_and_from_legacy_route() {
        let model = model_of(
            r#"{"name": "T", "rules": [
                {"name": "a", "type": "custom", "action": "reject", "domain": ["a.com"]},
                {"name": "b", "type": "custom", "action": "route", "outbound": "block", "domain": ["b.com"]}
            ]}"#,
        );
        assert_eq!(model.rules()[0].action, ActionView::Block);
        assert_eq!(model.rules()[1].action, ActionView::Block);
        // Reading changes nothing: the legacy rule stays as it was written.
        assert_eq!(model.to_share(), model_of(
            r#"{"name": "T", "rules": [
                {"name": "a", "type": "custom", "action": "reject", "domain": ["a.com"]},
                {"name": "b", "type": "custom", "action": "route", "outbound": "block", "domain": ["b.com"]}
            ]}"#,
        ).to_share());
        assert!(!model.is_dirty());
    }

    #[test]
    fn choosing_block_writes_reject_without_outbound() {
        let mut model = model_of(TWO);
        model.set_action(0, ActionView::Block).unwrap();
        let rule = &model.to_share().rules[0];
        assert_eq!(rule.action, "reject");
        assert_eq!(rule.outbound, None);
        // Untouched rules keep their shape.
        assert_eq!(model.to_share().rules[1], model_of(TWO).to_share().rules[1]);
    }

    #[test]
    fn simple_block_rule_is_a_reject_too() {
        let mut model = EditorModel::from_share(&synthetic(), false);
        model.set_action(3, ActionView::Block).unwrap();
        let rule = &model.to_share().rules[3];
        assert_eq!(rule.r#type, "simple_address_block");
        assert_eq!(rule.action, "reject");
        assert_eq!(rule.outbound, None);
    }

    #[test]
    fn moving_and_switching_keep_the_unknown_fields() {
        let share = synthetic();
        let mut model = EditorModel::from_share(&share, false);
        model.move_rule(5, true).unwrap();
        model.set_enabled(0, false).unwrap();
        model.set_enabled(0, true).unwrap();
        let back = model.to_share();
        assert_eq!(back.extra, share.extra);
        assert_eq!(back.default_outbound, share.default_outbound);
        // The endpoint rule moved up but is the same rule.
        assert_eq!(back.rules[4], share.rules[5]);
        assert_eq!(back.rules[5], share.rules[4]);
        assert_eq!(back.rules[4].extra["endpoint"], "wg0");
    }
}
