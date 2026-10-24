//! The route editor: a pure model of a routing profile for the two editing modes
//! of the screen. «Просто» works on three buckets of values ([`simple`]);
//! «Продвинуто» works on the ordered rule list ([`rules`], [`advanced`]).
//! Nothing here touches the UI or the network.
//!
//! The model edits the profile in place: it keeps the [`RouteShare`] it was built
//! from and changes only the fields an operation names. Everything else (`extra`
//! keys, unknown rule types and actions, disabled flags, outbound names the
//! editor does not know) comes back as it was read, so `to_share` of a fresh
//! model is the profile itself.

// The routing screen does not use the editor yet. Remove this when it does.
#![allow(dead_code, unused_imports)]

mod advanced;
mod rules;
mod simple;
mod values;

#[cfg(test)]
mod fixtures;

use throne_api::types::{Outbound, RouteRule, RouteShare};

pub use advanced::{CondKind, CondView};
pub use rules::{ActionView, Origin, RuleView};
pub use simple::{
    available_ready_lists, classify, Bucket, ReadyList, SimpleItem, SimpleKind, READY_LISTS,
};

/// A routing profile being edited.
#[derive(Debug, Clone)]
pub struct EditorModel {
    /// The profile as it was read. [`EditorModel::is_dirty`] compares against it.
    original: RouteShare,
    /// The profile as it is being edited.
    share: RouteShare,
    /// The profile follows a remote URL. The screen saves it as a copy.
    is_remote: bool,
}

impl EditorModel {
    /// Starts editing `share`. `is_remote` is the route's `remote` flag.
    pub fn from_share(share: &RouteShare, is_remote: bool) -> Self {
        Self {
            original: share.clone(),
            share: share.clone(),
            is_remote,
        }
    }

    /// The profile as it is now, ready for `PUT /v1/routes/{id}` or a link.
    pub fn to_share(&self) -> RouteShare {
        self.share.clone()
    }

    /// Whether the profile differs from the one the model was built from.
    pub fn is_dirty(&self) -> bool {
        self.share != self.original
    }

    /// Whether the profile follows a remote URL.
    pub fn is_remote(&self) -> bool {
        self.is_remote
    }

    /// The profile name.
    pub fn name(&self) -> &str {
        &self.share.name
    }

    /// Renames the profile. The name is trimmed.
    pub fn set_name(&mut self, name: &str) {
        self.share.name = name.trim().to_owned();
    }

    /// The outbound for traffic no rule matches.
    pub fn default_outbound(&self) -> &Outbound {
        &self.share.default_outbound
    }

    /// Sets the outbound for traffic no rule matches. The screen offers VPN and
    /// direct only; a profile that has block or another outbound keeps it until
    /// this is called.
    pub fn set_default_outbound(&mut self, outbound: Outbound) -> Result<(), String> {
        match outbound {
            Outbound::Proxy | Outbound::Direct => {
                self.share.default_outbound = outbound;
                Ok(())
            }
            _ => Err("По умолчанию можно выбрать только VPN или напрямую".to_owned()),
        }
    }

    /// The problems that stop the profile from being saved, in Russian. Empty
    /// when the profile is valid.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.share.name.trim().is_empty() {
            problems.push("Укажите название маршрута".to_owned());
        }
        for (index, rule) in self.share.rules.iter().enumerate() {
            let title = rule_title(index, rule);
            if rule.r#type == "custom" {
                if rule.name.trim().is_empty() {
                    problems.push(format!("Правило №{} без названия", index + 1));
                }
                if rule.action == "route" && !advanced::has_conditions(rule) {
                    problems.push(format!(
                        "Правило {title} без условий: оно будет действовать на весь трафик"
                    ));
                }
                if rule.action == "route" && rule.outbound.is_none() {
                    problems.push(format!("Правило {title} не знает, куда вести трафик"));
                }
            }
            for cond in advanced::present_conditions(rule) {
                if cond.values.is_empty() {
                    problems.push(format!(
                        "В правиле {title} условие «{}» без значений",
                        cond.kind.label()
                    ));
                }
            }
        }
        problems
    }

    /// The rule at `index`, for an edit.
    fn rule_mut(&mut self, index: usize) -> Result<&mut RouteRule, String> {
        let len = self.share.rules.len();
        self.share
            .rules
            .get_mut(index)
            .ok_or_else(|| missing_rule(index, len))
    }

    /// Fails unless `index` is a rule of the profile.
    fn check_rule(&self, index: usize) -> Result<(), String> {
        let len = self.share.rules.len();
        if index < len {
            Ok(())
        } else {
            Err(missing_rule(index, len))
        }
    }
}

/// The name a remote profile is saved under when the user keeps it as a copy.
pub fn copy_name(name: &str) -> String {
    format!("{} (копия)", name.trim())
}

/// How a rule is named in a message: its name in quotes, or its number.
fn rule_title(index: usize, rule: &RouteRule) -> String {
    let name = rule.name.trim();
    if name.is_empty() {
        format!("№{}", index + 1)
    } else {
        format!("«{name}»")
    }
}

fn missing_rule(index: usize, len: usize) -> String {
    format!("Нет правила №{} (всего правил: {len})", index + 1)
}

#[cfg(test)]
mod tests {
    use super::fixtures::{bypass_russia, model_of, synthetic};
    use super::*;

    #[test]
    fn bypass_russia_round_trips_unchanged() {
        let share = bypass_russia();
        let model = EditorModel::from_share(&share, true);
        assert_eq!(model.to_share(), share);
        assert!(!model.is_dirty());
        assert!(model.is_remote());
    }

    #[test]
    fn synthetic_profile_round_trips_unchanged() {
        let share = synthetic();
        let model = EditorModel::from_share(&share, false);
        assert_eq!(model.to_share(), share);
        assert!(!model.is_remote());
    }

    #[test]
    fn unknown_keys_keep_their_values_on_the_way_back() {
        let back = EditorModel::from_share(&synthetic(), false).to_share();
        let json = serde_json::to_value(&back).expect("the profile serializes");
        assert_eq!(json["dns"]["final"], "remote");
        assert_eq!(json["raw"]["log"]["level"], "warn");
        assert_eq!(
            json["outbound_names"],
            serde_json::json!(["proxy", "direct"])
        );
        assert_eq!(json["rules"][0]["sniffer_hint"], 1);
        assert_eq!(json["rules"][5]["endpoint"], "wg0");
        assert_eq!(json["rules"][6]["method"], "drop");
        assert_eq!(json["rules"][8]["source_port"], serde_json::json!([1]));
        assert_eq!(json["rules"][2]["disabled"], true);
        assert_eq!(json["rules"][8]["outbound"], "proxy-b");
    }

    #[test]
    fn bypass_russia_is_not_dirty_after_a_no_op_edit() {
        let share = bypass_russia();
        let mut model = EditorModel::from_share(&share, false);
        model.set_name(&share.name);
        model.set_default_outbound(Outbound::Proxy).unwrap();
        assert!(!model.is_dirty());
    }

    #[test]
    fn an_edit_makes_it_dirty_and_reverting_clears_it() {
        let share = synthetic();
        let mut model = EditorModel::from_share(&share, false);
        model.set_name("Другое");
        assert!(model.is_dirty());
        model.set_name(&share.name);
        assert!(!model.is_dirty());
    }

    #[test]
    fn name_and_default_outbound() {
        let mut model = EditorModel::from_share(&synthetic(), false);
        assert_eq!(model.name(), "Synthetic");
        assert_eq!(model.default_outbound(), &Outbound::Direct);
        model.set_name("  Дом ");
        assert_eq!(model.name(), "Дом");
        model.set_default_outbound(Outbound::Proxy).unwrap();
        assert_eq!(model.to_share().default_outbound, Outbound::Proxy);
        assert!(model.set_default_outbound(Outbound::Block).is_err());
        assert!(model.set_default_outbound(Outbound::WarpBypass).is_err());
        assert_eq!(model.default_outbound(), &Outbound::Proxy);
    }

    #[test]
    fn block_default_is_kept_until_the_screen_changes_it() {
        let share: RouteShare =
            serde_json::from_str(r#"{"name": "T", "default_outbound": "block", "rules": []}"#)
                .expect("parses");
        let model = EditorModel::from_share(&share, false);
        assert_eq!(model.to_share(), share);
        assert_eq!(model.default_outbound(), &Outbound::Block);
    }

    #[test]
    fn edits_leave_the_other_fields_alone() {
        let share = synthetic();
        let mut model = EditorModel::from_share(&share, false);
        model.add_rule();
        model.add_simple(Bucket::Proxy, "example.com").unwrap();
        model.add_condition(0, CondKind::Port).unwrap();
        model.set_enabled(1, false).unwrap();
        let back = model.to_share();
        assert_eq!(back.extra, share.extra);
        assert_eq!(back.kind, share.kind);
        assert_eq!(back.v, share.v);
        assert_eq!(back.default_outbound, share.default_outbound);
        assert_eq!(back.rules[7], share.rules[7]);
        assert_eq!(back.rules[5], share.rules[5]);
    }

    #[test]
    fn copy_name_adds_the_suffix() {
        assert_eq!(copy_name(" Bypass Russia "), "Bypass Russia (копия)");
    }

    #[test]
    fn bypass_russia_is_valid() {
        assert!(EditorModel::from_share(&bypass_russia(), false)
            .validate()
            .is_empty());
    }

    #[test]
    fn validate_reports_the_problems_of_a_new_rule() {
        let mut model = model_of(r#"{"name": "T", "rules": []}"#);
        let index = model.add_rule();
        let problems = model.validate();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("без условий"), "{problems:?}");

        model.add_value(index, CondKind::Port, "443").unwrap();
        assert!(model.validate().is_empty());

        model.add_condition(index, CondKind::Domain).unwrap();
        let problems = model.validate();
        assert_eq!(
            problems,
            ["В правиле «Новое правило» условие «Сайт (точно)» без значений"]
        );
        model.remove_condition(index, CondKind::Domain).unwrap();

        model.rename_rule(index, "  ").unwrap();
        assert_eq!(model.validate(), ["Правило №1 без названия"]);
    }

    #[test]
    fn validate_flags_an_empty_name_and_empty_profile() {
        let mut model = EditorModel::from_share(&bypass_russia(), false);
        model.set_name("   ");
        assert_eq!(model.validate(), ["Укажите название маршрута"]);
    }

    #[test]
    fn simple_rules_without_a_name_are_valid() {
        let model = model_of(
            r#"{"name": "T", "rules": [{"type": "simple_address_block", "action": "route", "outbound": "block", "domain_suffix": ["a.com"]}]}"#,
        );
        assert!(model.validate().is_empty());
    }
}
