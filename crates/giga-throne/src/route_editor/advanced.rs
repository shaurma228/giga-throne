//! Conditions of a rule in the advanced mode: which fields of a rule are set,
//! the values they hold, and the one-line summary the rule list shows.
//!
//! A condition is present when its field is set. A list condition with no
//! values is a fresh one the user has not filled yet; [`EditorModel::validate`]
//! reports it.

use throne_api::types::RouteRule;

use super::rules::is_editable;
use super::values::normalize_value;
use super::EditorModel;

/// Shown when a flag condition gets a value of text.
const FLAG_HINT: &str = "Для этого условия выберите да или нет";

/// A kind of condition. Each one is a field of a route rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CondKind {
    Domain,
    DomainSuffix,
    DomainKeyword,
    DomainRegex,
    IpCidr,
    IpIsPrivate,
    SourceIpCidr,
    Port,
    PortRange,
    ProcessName,
    ProcessPath,
    RuleSet,
    Network,
    Protocol,
    Invert,
}

impl CondKind {
    /// Every kind, in the order the condition picker and the summary use.
    pub const ALL: [CondKind; 15] = [
        CondKind::Domain,
        CondKind::DomainSuffix,
        CondKind::DomainKeyword,
        CondKind::DomainRegex,
        CondKind::IpCidr,
        CondKind::IpIsPrivate,
        CondKind::SourceIpCidr,
        CondKind::Port,
        CondKind::PortRange,
        CondKind::ProcessName,
        CondKind::ProcessPath,
        CondKind::RuleSet,
        CondKind::Network,
        CondKind::Protocol,
        CondKind::Invert,
    ];

    /// The label in the condition picker.
    pub fn label(self) -> &'static str {
        match self {
            CondKind::Domain => "Сайт (точно)",
            CondKind::DomainSuffix => "Сайт оканчивается на",
            CondKind::DomainKeyword => "Сайт содержит",
            CondKind::DomainRegex => "Сайт по шаблону (regex)",
            CondKind::IpCidr => "IP или подсеть",
            CondKind::IpIsPrivate => "Частные IP (локальная сеть)",
            CondKind::SourceIpCidr => "IP источника",
            CondKind::Port => "Порт",
            CondKind::PortRange => "Диапазон портов",
            CondKind::ProcessName => "Программа",
            CondKind::ProcessPath => "Путь программы",
            CondKind::RuleSet => "Список (rule-set)",
            CondKind::Network => "Сеть (tcp/udp)",
            CondKind::Protocol => "Протокол (dns, tls, quic…)",
            CondKind::Invert => "Наоборот (если НЕ подходит)",
        }
    }

    /// The short form the rule summary uses.
    fn short_label(self) -> &'static str {
        match self {
            CondKind::Domain => "сайт",
            CondKind::DomainSuffix => "сайт оканчивается на",
            CondKind::DomainKeyword => "сайт содержит",
            CondKind::DomainRegex => "сайт по шаблону",
            CondKind::IpCidr => "IP",
            CondKind::IpIsPrivate => "частный IP",
            CondKind::SourceIpCidr => "IP источника",
            CondKind::Port => "порт",
            CondKind::PortRange => "диапазон портов",
            CondKind::ProcessName => "программа",
            CondKind::ProcessPath => "путь программы",
            CondKind::RuleSet => "список",
            CondKind::Network => "сеть",
            CondKind::Protocol => "протокол",
            CondKind::Invert => "наоборот",
        }
    }

    /// On or off, set with [`EditorModel::set_flag`].
    pub fn is_flag(self) -> bool {
        matches!(self, CondKind::IpIsPrivate | CondKind::Invert)
    }
}

/// A condition of a rule with the values it holds. Flags hold `да` or `нет`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondView {
    pub kind: CondKind,
    pub values: Vec<String>,
}

impl EditorModel {
    /// The conditions a rule has, in [`CondKind::ALL`] order. Empty for a bad index.
    pub fn conditions(&self, index: usize) -> Vec<CondView> {
        self.share
            .rules
            .get(index)
            .map(present_conditions)
            .unwrap_or_default()
    }

    /// Adds an empty condition of `kind`. A flag becomes `да`. Does nothing if the
    /// rule already has it.
    pub fn add_condition(&mut self, index: usize, kind: CondKind) -> Result<(), String> {
        let rule = self.editable_rule_mut(index)?;
        if is_present(rule, kind) {
            return Ok(());
        }
        match kind {
            CondKind::IpIsPrivate => rule.ip_is_private = Some(true),
            CondKind::Invert => rule.invert = Some(true),
            CondKind::Network => rule.network = Some(String::new()),
            CondKind::Protocol => rule.protocol = Some(String::new()),
            _ => {
                if let Some(field) = list_field_mut(rule, kind) {
                    *field = Some(Vec::new());
                }
            }
        }
        Ok(())
    }

    /// Removes a condition and its values.
    pub fn remove_condition(&mut self, index: usize, kind: CondKind) -> Result<(), String> {
        let rule = self.editable_rule_mut(index)?;
        clear_condition(rule, kind);
        Ok(())
    }

    /// Adds a value to a condition, which is created if needed. The value is
    /// checked for its kind and stored normalized. A single-value condition
    /// (network, protocol) takes the new value in place of the old one.
    pub fn add_value(&mut self, index: usize, kind: CondKind, value: &str) -> Result<(), String> {
        let value = normalize_value(kind, value)?;
        let rule = self.editable_rule_mut(index)?;
        match kind {
            CondKind::Network => rule.network = Some(value),
            CondKind::Protocol => rule.protocol = Some(value),
            _ => {
                if let Some(field) = list_field_mut(rule, kind) {
                    let list = field.get_or_insert_with(Vec::new);
                    if !list.contains(&value) {
                        list.push(value);
                    }
                }
            }
        }
        Ok(())
    }

    /// Removes one value, as the condition shows it. A list condition keeps
    /// existing without values until it is removed itself; a single-value
    /// condition goes with its value.
    pub fn remove_value(
        &mut self,
        index: usize,
        kind: CondKind,
        value: &str,
    ) -> Result<(), String> {
        let rule = self.editable_rule_mut(index)?;
        match kind {
            CondKind::Network => {
                if rule.network.as_deref() == Some(value) {
                    rule.network = None;
                }
            }
            CondKind::Protocol => {
                if rule.protocol.as_deref() == Some(value) {
                    rule.protocol = None;
                }
            }
            CondKind::IpIsPrivate | CondKind::Invert => return Err(FLAG_HINT.to_owned()),
            _ => {
                if let Some(list) = list_field_mut(rule, kind).and_then(Option::as_mut) {
                    list.retain(|item| item.as_str() != value);
                }
            }
        }
        Ok(())
    }

    /// Sets a flag condition (`IpIsPrivate` or `Invert`) to `value`.
    pub fn set_flag(&mut self, index: usize, kind: CondKind, value: bool) -> Result<(), String> {
        let rule = self.editable_rule_mut(index)?;
        match kind {
            CondKind::IpIsPrivate => rule.ip_is_private = Some(value),
            CondKind::Invert => rule.invert = Some(value),
            _ => return Err("Это условие не переключается".to_owned()),
        }
        Ok(())
    }

    /// The rule at `index`, if its conditions may be edited here. Only custom
    /// rules and the simple ones are; the other types are shown read-only.
    fn editable_rule_mut(&mut self, index: usize) -> Result<&mut RouteRule, String> {
        let rule = self.rule_mut(index)?;
        if is_editable(rule) {
            Ok(rule)
        } else {
            Err("Условия этого правила здесь не меняются".to_owned())
        }
    }
}

/// The list field of a rule that `kind` names. `None` for the single-value and
/// flag kinds.
pub(super) fn list_field_mut(
    rule: &mut RouteRule,
    kind: CondKind,
) -> Option<&mut Option<Vec<String>>> {
    match kind {
        CondKind::Domain => Some(&mut rule.domain),
        CondKind::DomainSuffix => Some(&mut rule.domain_suffix),
        CondKind::DomainKeyword => Some(&mut rule.domain_keyword),
        CondKind::DomainRegex => Some(&mut rule.domain_regex),
        CondKind::IpCidr => Some(&mut rule.ip_cidr),
        CondKind::SourceIpCidr => Some(&mut rule.source_ip_cidr),
        CondKind::Port => Some(&mut rule.port),
        CondKind::PortRange => Some(&mut rule.port_range),
        CondKind::ProcessName => Some(&mut rule.process_name),
        CondKind::ProcessPath => Some(&mut rule.process_path),
        CondKind::RuleSet => Some(&mut rule.rule_set),
        CondKind::IpIsPrivate | CondKind::Network | CondKind::Protocol | CondKind::Invert => None,
    }
}

/// The list field of a rule that `kind` names, read-only. `None` for the
/// single-value and flag kinds.
fn list_field(rule: &RouteRule, kind: CondKind) -> Option<&Option<Vec<String>>> {
    match kind {
        CondKind::Domain => Some(&rule.domain),
        CondKind::DomainSuffix => Some(&rule.domain_suffix),
        CondKind::DomainKeyword => Some(&rule.domain_keyword),
        CondKind::DomainRegex => Some(&rule.domain_regex),
        CondKind::IpCidr => Some(&rule.ip_cidr),
        CondKind::SourceIpCidr => Some(&rule.source_ip_cidr),
        CondKind::Port => Some(&rule.port),
        CondKind::PortRange => Some(&rule.port_range),
        CondKind::ProcessName => Some(&rule.process_name),
        CondKind::ProcessPath => Some(&rule.process_path),
        CondKind::RuleSet => Some(&rule.rule_set),
        CondKind::IpIsPrivate | CondKind::Network | CondKind::Protocol | CondKind::Invert => None,
    }
}

/// The values of a list condition of a rule; empty when it is not set.
pub(super) fn list_values(rule: &RouteRule, kind: CondKind) -> &[String] {
    list_field(rule, kind)
        .and_then(|field| field.as_deref())
        .unwrap_or(&[])
}

/// Whether the rule has the condition `kind` at all.
fn is_present(rule: &RouteRule, kind: CondKind) -> bool {
    match kind {
        CondKind::IpIsPrivate => rule.ip_is_private.is_some(),
        CondKind::Invert => rule.invert.is_some(),
        CondKind::Network => rule.network.is_some(),
        CondKind::Protocol => rule.protocol.is_some(),
        _ => list_field(rule, kind).is_some_and(Option::is_some),
    }
}

/// Removes a condition: its field goes back to unset.
fn clear_condition(rule: &mut RouteRule, kind: CondKind) {
    match kind {
        CondKind::IpIsPrivate => rule.ip_is_private = None,
        CondKind::Invert => rule.invert = None,
        CondKind::Network => rule.network = None,
        CondKind::Protocol => rule.protocol = None,
        _ => {
            if let Some(field) = list_field_mut(rule, kind) {
                *field = None;
            }
        }
    }
}

/// The value of a flag condition, `None` when unset or not a flag.
fn flag_value(rule: &RouteRule, kind: CondKind) -> Option<bool> {
    match kind {
        CondKind::IpIsPrivate => rule.ip_is_private,
        CondKind::Invert => rule.invert,
        _ => None,
    }
}

/// The values a condition shows. Flags show `да` or `нет`.
fn values_of(rule: &RouteRule, kind: CondKind) -> Vec<String> {
    match kind {
        CondKind::IpIsPrivate | CondKind::Invert => flag_value(rule, kind)
            .map(|on| if on { "да" } else { "нет" }.to_owned())
            .into_iter()
            .collect(),
        CondKind::Network => rule
            .network
            .iter()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect(),
        CondKind::Protocol => rule
            .protocol
            .iter()
            .filter(|v| !v.is_empty())
            .cloned()
            .collect(),
        _ => list_values(rule, kind).to_vec(),
    }
}

/// The conditions a rule has, with their values.
pub(super) fn present_conditions(rule: &RouteRule) -> Vec<CondView> {
    CondKind::ALL
        .into_iter()
        .filter(|kind| is_present(rule, *kind))
        .map(|kind| CondView {
            kind,
            values: values_of(rule, kind),
        })
        .collect()
}

/// Whether the rule has any condition.
pub(super) fn has_conditions(rule: &RouteRule) -> bool {
    CondKind::ALL.into_iter().any(|kind| is_present(rule, kind))
}

/// The one-line summary of a rule, for example
/// `сайт оканчивается на: a.com, b.com · порт: 443`.
pub(super) fn summary(rule: &RouteRule) -> String {
    let parts: Vec<String> = CondKind::ALL
        .into_iter()
        .filter(|kind| is_present(rule, *kind))
        .filter_map(|kind| summary_part(rule, kind))
        .collect();
    if !parts.is_empty() {
        parts.join(" · ")
    } else if is_editable(rule) {
        "нет условий".to_owned()
    } else {
        format!("тип: {}", rule.r#type)
    }
}

/// One part of the summary. `None` for a flag that is off and a list with no values.
fn summary_part(rule: &RouteRule, kind: CondKind) -> Option<String> {
    let label = kind.short_label();
    if kind.is_flag() {
        return match flag_value(rule, kind)? {
            true => Some(label.to_owned()),
            false => Some(format!("{label}: нет")),
        };
    }
    let values = values_of(rule, kind);
    (!values.is_empty()).then(|| format!("{label}: {}", values.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{model_of, synthetic};
    use super::super::EditorModel;
    use super::*;
    use throne_api::types::{Outbound, RouteShare};

    const PLAIN: &str = r#"{"name": "T", "rules": [{"name": "r", "type": "custom", "action": "route", "outbound": "proxy"}]}"#;

    fn values(model: &EditorModel, index: usize, kind: CondKind) -> Vec<String> {
        model
            .conditions(index)
            .into_iter()
            .find(|cond| cond.kind == kind)
            .map(|cond| cond.values)
            .unwrap_or_default()
    }

    #[test]
    fn a_new_condition_is_empty_until_filled() {
        let mut model = model_of(PLAIN);
        model.add_condition(0, CondKind::Domain).unwrap();
        assert_eq!(
            model.conditions(0),
            vec![CondView {
                kind: CondKind::Domain,
                values: vec![]
            }]
        );
        model.add_condition(0, CondKind::Domain).unwrap();
        assert_eq!(model.conditions(0).len(), 1);
    }

    #[test]
    fn add_value_normalizes_and_dedupes() {
        let mut model = model_of(PLAIN);
        model
            .add_value(0, CondKind::DomainSuffix, "Ex.COM")
            .unwrap();
        model
            .add_value(0, CondKind::DomainSuffix, "ex.com")
            .unwrap();
        model.add_value(0, CondKind::DomainKeyword, "ADS").unwrap();
        model.add_value(0, CondKind::IpCidr, "1.2.3.4").unwrap();
        model
            .add_value(0, CondKind::ProcessName, "steam.exe")
            .unwrap();
        model.add_value(0, CondKind::RuleSet, "geosite-ru").unwrap();
        assert_eq!(values(&model, 0, CondKind::DomainSuffix), vec!["ex.com"]);
        assert_eq!(values(&model, 0, CondKind::DomainKeyword), vec!["ads"]);
        assert_eq!(values(&model, 0, CondKind::IpCidr), vec!["1.2.3.4/32"]);
        assert_eq!(values(&model, 0, CondKind::ProcessName), vec!["steam.exe"]);
        assert_eq!(values(&model, 0, CondKind::RuleSet), vec!["geosite-ru"]);
    }

    #[test]
    fn ports_and_ranges_are_checked() {
        let mut model = model_of(PLAIN);
        model.add_value(0, CondKind::Port, " 443 ").unwrap();
        assert!(model.add_value(0, CondKind::Port, "0").is_err());
        assert!(model.add_value(0, CondKind::Port, "70000").is_err());
        model
            .add_value(0, CondKind::PortRange, "27015-27050")
            .unwrap();
        assert!(model.add_value(0, CondKind::PortRange, "5:1").is_err());
        assert_eq!(values(&model, 0, CondKind::Port), vec!["443"]);
        assert_eq!(values(&model, 0, CondKind::PortRange), vec!["27015:27050"]);
    }

    #[test]
    fn other_kinds_are_checked() {
        let mut model = model_of(PLAIN);
        assert!(model.add_value(0, CondKind::IpCidr, "1.2.3.4/40").is_err());
        assert!(model.add_value(0, CondKind::DomainSuffix, "1.2.3").is_err());
        assert!(model.add_value(0, CondKind::DomainRegex, "").is_err());
        model
            .add_value(0, CondKind::DomainRegex, "^ad[0-9]+")
            .unwrap();
        assert!(model
            .add_value(0, CondKind::ProcessName, "Google Chrome")
            .is_err());
        assert!(model.add_value(0, CondKind::RuleSet, "geo site").is_err());
        assert!(model.add_value(0, CondKind::ProcessPath, "").is_err());
        model
            .add_value(0, CondKind::ProcessPath, "/usr/bin/foo")
            .unwrap();
        assert!(model.add_value(0, CondKind::IpIsPrivate, "да").is_err());
        assert!(model.add_value(0, CondKind::Invert, "true").is_err());
    }

    #[test]
    fn single_value_conditions_take_one_value() {
        let mut model = model_of(PLAIN);
        model.add_value(0, CondKind::Network, "TCP").unwrap();
        model.add_value(0, CondKind::Network, "udp").unwrap();
        assert_eq!(values(&model, 0, CondKind::Network), vec!["udp"]);
        assert!(model.add_value(0, CondKind::Network, "icmp").is_err());
        model.add_value(0, CondKind::Protocol, "dns").unwrap();
        assert!(model.add_value(0, CondKind::Protocol, "dns!").is_err());
        assert_eq!(values(&model, 0, CondKind::Protocol), vec!["dns"]);
        // A single-value condition goes with its value.
        model.remove_value(0, CondKind::Network, "udp").unwrap();
        assert!(model
            .conditions(0)
            .iter()
            .all(|c| c.kind != CondKind::Network));
    }

    #[test]
    fn flags_are_set_and_shown_as_yes_or_no() {
        let mut model = model_of(PLAIN);
        model.add_condition(0, CondKind::IpIsPrivate).unwrap();
        assert_eq!(values(&model, 0, CondKind::IpIsPrivate), vec!["да"]);
        model.set_flag(0, CondKind::IpIsPrivate, false).unwrap();
        assert_eq!(values(&model, 0, CondKind::IpIsPrivate), vec!["нет"]);
        model.set_flag(0, CondKind::Invert, true).unwrap();
        assert_eq!(model.to_share().rules[0].invert, Some(true));
        assert!(model.set_flag(0, CondKind::Domain, true).is_err());
        assert!(model.remove_value(0, CondKind::Invert, "да").is_err());
    }

    #[test]
    fn remove_value_and_remove_condition() {
        let mut model = model_of(PLAIN);
        model.add_value(0, CondKind::Domain, "a.com").unwrap();
        model.add_value(0, CondKind::Domain, "b.com").unwrap();
        model.remove_value(0, CondKind::Domain, "a.com").unwrap();
        assert_eq!(values(&model, 0, CondKind::Domain), vec!["b.com"]);
        model.remove_value(0, CondKind::Domain, "b.com").unwrap();
        assert!(model
            .conditions(0)
            .iter()
            .any(|c| c.kind == CondKind::Domain));
        model.remove_condition(0, CondKind::Domain).unwrap();
        assert!(model.conditions(0).is_empty());
        model.remove_condition(0, CondKind::Domain).unwrap();
    }

    #[test]
    fn conditions_of_opaque_rules_are_read_only() {
        let mut model = EditorModel::from_share(&synthetic(), false);
        // Index 4 is a process-path rule, index 5 an endpoint rule.
        for index in [4, 5] {
            assert!(model.add_condition(index, CondKind::Domain).is_err());
            assert!(model.add_value(index, CondKind::Domain, "a.com").is_err());
            assert!(model.remove_condition(index, CondKind::Domain).is_err());
            assert!(model.set_flag(index, CondKind::Invert, true).is_err());
        }
        assert_eq!(model.conditions(4).len(), 1);
    }

    #[test]
    fn summary_reads_like_the_design() {
        let mut model = model_of(PLAIN);
        model.add_value(0, CondKind::DomainSuffix, "a.com").unwrap();
        model.add_value(0, CondKind::DomainSuffix, "b.com").unwrap();
        model.add_value(0, CondKind::Port, "443").unwrap();
        assert_eq!(
            model.rules()[0].summary,
            "сайт оканчивается на: a.com, b.com · порт: 443"
        );
    }

    #[test]
    fn summary_of_a_game_rule() {
        let mut model = model_of(PLAIN);
        model
            .add_value(0, CondKind::ProcessName, "steam.exe")
            .unwrap();
        model.add_value(0, CondKind::ProcessName, "cs2").unwrap();
        model
            .add_value(0, CondKind::PortRange, "27015-27050")
            .unwrap();
        model.set_flag(0, CondKind::Invert, false).unwrap();
        assert_eq!(
            model.rules()[0].summary,
            "диапазон портов: 27015:27050 · программа: steam.exe, cs2 · наоборот: нет"
        );
    }

    #[test]
    fn summary_of_the_synthetic_profile() {
        let model = EditorModel::from_share(&synthetic(), false);
        let rules = model.rules();
        assert_eq!(
            rules[1].summary,
            "сайт оканчивается на: ads.example.com · список: geosite-category-ads-all"
        );
        assert_eq!(rules[4].summary, "путь программы: /usr/bin/foo");
        assert_eq!(rules[5].summary, "тип: endpoint_preferred_by");
        assert_eq!(rules[6].summary, "сайт содержит: ads");
        assert_eq!(
            rules[7].summary,
            "порт: 443, 8080 · диапазон портов: 1000:2000 · сеть: tcp · наоборот"
        );
        assert_eq!(rules[8].summary, "частный IP: нет");
        assert_eq!(
            rules[8].action,
            super::super::ActionView::Other("proxy-b".to_owned())
        );
    }

    #[test]
    fn outbound_names_survive_condition_edits() {
        let share: RouteShare = synthetic();
        let mut model = EditorModel::from_share(&share, false);
        model.add_value(7, CondKind::Protocol, "dns").unwrap();
        assert_eq!(
            model.to_share().rules[7].outbound,
            Some(Outbound::Unknown("custom-out".to_owned()))
        );
    }
}
