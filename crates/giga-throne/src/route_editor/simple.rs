//! The simple mode: three buckets of values, each one outbound.
//!
//! A bucket's addresses live in one `simple_address_*` rule and its program
//! names in one `simple_process_name_*` rule. Its type token says the bucket:
//! `proxy`, `bypass` (Напрямую) or `block`. A value moved into another bucket
//! leaves the old one. Simple rules come before the custom rules; a new one
//! goes where its bucket's order puts it: block, then direct, then proxy.

use throne_api::types::{Outbound, RouteRule};

use super::advanced::{has_conditions, list_field_mut, list_values, present_conditions, CondKind};
use super::rules::new_rule;
use super::values::{
    domain_for_display, is_process_name, is_rule_set_name, normalize_cidr, normalize_host,
    normalize_value,
};
use super::EditorModel;

/// What the classifier says about a value that is none of the known forms.
const NOT_RECOGNIZED: &str = "Не похоже на сайт, IP или программу";

/// One of the three buckets of the simple mode. Each is an outbound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// «Через VPN», the proxy outbound.
    Proxy,
    /// «Напрямую», the direct outbound (Throne's `bypass` rules).
    Direct,
    /// «Блокировать», the block outbound.
    Block,
}

impl Bucket {
    /// The buckets in the order the screen shows them.
    pub const ALL: [Bucket; 3] = [Bucket::Proxy, Bucket::Direct, Bucket::Block];

    /// The title on the screen.
    pub fn title(self) -> &'static str {
        match self {
            Bucket::Proxy => "Через VPN",
            Bucket::Direct => "Напрямую",
            Bucket::Block => "Блокировать",
        }
    }

    /// The action of the bucket's rules, as Throne writes it: `route` for VPN and
    /// direct, `reject` for block.
    pub fn action(self) -> &'static str {
        match self {
            Bucket::Proxy | Bucket::Direct => "route",
            Bucket::Block => "reject",
        }
    }

    /// The outbound of the bucket's rules. Throne gives block rules none.
    pub fn outbound(self) -> Option<Outbound> {
        match self {
            Bucket::Proxy => Some(Outbound::Proxy),
            Bucket::Direct => Some(Outbound::Direct),
            Bucket::Block => None,
        }
    }

    /// Where the bucket's simple rules go among each other: lower comes first.
    fn rank(self) -> u8 {
        match self {
            Bucket::Block => 0,
            Bucket::Direct => 1,
            Bucket::Proxy => 2,
        }
    }
}

/// What a simple rule matches: addresses, or program names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Address,
    Process,
}

impl Family {
    fn title(self) -> &'static str {
        match self {
            Family::Address => "Сайты и IP",
            Family::Process => "Программы",
        }
    }
}

/// The kind of a value in the simple mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleKind {
    /// Exact domain (`full:`).
    Domain,
    /// Domain with its subdomains.
    DomainSuffix,
    /// A word in domains (`keyword:`).
    DomainKeyword,
    /// A pattern over domains (`regex:`).
    DomainRegex,
    /// IP address or CIDR range.
    IpCidr,
    /// Rule-set name, or a ready-made list as `list:<name>`.
    RuleSet,
    /// Program name.
    ProcessName,
}

impl SimpleKind {
    /// The condition, that is the rule field, the value is stored in.
    pub fn cond(self) -> CondKind {
        match self {
            SimpleKind::Domain => CondKind::Domain,
            SimpleKind::DomainSuffix => CondKind::DomainSuffix,
            SimpleKind::DomainKeyword => CondKind::DomainKeyword,
            SimpleKind::DomainRegex => CondKind::DomainRegex,
            SimpleKind::IpCidr => CondKind::IpCidr,
            SimpleKind::RuleSet => CondKind::RuleSet,
            SimpleKind::ProcessName => CondKind::ProcessName,
        }
    }

    /// The short tag a chip shows.
    pub fn label(self) -> &'static str {
        match self {
            SimpleKind::Domain => "Точно",
            SimpleKind::DomainSuffix => "Сайт",
            SimpleKind::DomainKeyword => "Содержит",
            SimpleKind::DomainRegex => "Шаблон",
            SimpleKind::IpCidr => "IP",
            SimpleKind::RuleSet => "Список",
            SimpleKind::ProcessName => "Программа",
        }
    }

    fn family(self) -> Family {
        match self {
            SimpleKind::ProcessName => Family::Process,
            _ => Family::Address,
        }
    }
}

/// One value of a bucket, as a chip shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleItem {
    pub kind: SimpleKind,
    /// The value as stored. A ready-made list is `list:<name>`.
    pub value: String,
    /// The short tag, the kind's [`SimpleKind::label`].
    pub label: &'static str,
}

/// A named group of Throne rule-sets that one bucket takes in one step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyList {
    pub name: &'static str,
    pub bucket: Bucket,
    pub rule_sets: &'static [&'static str],
}

/// The ready-made lists the screen offers.
pub static READY_LISTS: [ReadyList; 4] = [
    ReadyList {
        name: "Заблокированное в РФ",
        bucket: Bucket::Proxy,
        rule_sets: &["geosite-ru-blocked", "geoip-ru-blocked"],
    },
    ReadyList {
        name: "Российские сайты",
        bucket: Bucket::Direct,
        rule_sets: &["geosite-ru", "geoip-ru", "geosite-category-ru"],
    },
    ReadyList {
        name: "Реклама",
        bucket: Bucket::Block,
        rule_sets: &["geosite-category-ads-all"],
    },
    ReadyList {
        name: "Стриминг",
        bucket: Bucket::Proxy,
        rule_sets: &["geosite-netflix", "geosite-youtube", "geosite-spotify"],
    },
];

/// The ready-made lists to offer. A list is hidden when one of its rule-sets is
/// not in `known_rule_sets`; `None` means the names are unknown and every list
/// is offered.
pub fn available_ready_lists(known_rule_sets: Option<&[String]>) -> Vec<ReadyList> {
    READY_LISTS
        .iter()
        .copied()
        .filter(|list| match known_rule_sets {
            None => true,
            Some(known) => list
                .rule_sets
                .iter()
                .all(|name| known.iter().any(|held| held.as_str() == *name)),
        })
        .collect()
}

/// Works out what a typed value is and the value to store.
///
/// Prefixes pick the kind: `list:<name>` a ready-made list, `full:` an exact
/// domain, `keyword:` a word in domains, `regex:` a pattern. Without a prefix an
/// IP or CIDR range, a rule-set name (`geosite-…`, `geoip-…`), a program name
/// (no dot, or `.exe`) and a domain (which includes its subdomains) are tried in
/// that order.
pub fn classify(input: &str) -> Result<(SimpleKind, String), String> {
    let text = input.trim();
    if let Some(name) = text.strip_prefix("list:") {
        let list = ready_list(name)?;
        return Ok((SimpleKind::RuleSet, format!("list:{}", list.name)));
    }
    let prefixed = [
        ("full:", SimpleKind::Domain),
        ("keyword:", SimpleKind::DomainKeyword),
        ("regex:", SimpleKind::DomainRegex),
    ];
    for (prefix, kind) in prefixed {
        if let Some(rest) = text.strip_prefix(prefix) {
            return normalize_value(kind.cond(), rest).map(|value| (kind, value));
        }
    }
    if let Some(cidr) = normalize_cidr(text) {
        return Ok((SimpleKind::IpCidr, cidr));
    }
    let lower = text.to_ascii_lowercase();
    if is_rule_set_name(&lower) {
        return Ok((SimpleKind::RuleSet, lower));
    }
    if is_process_name(text) {
        return Ok((SimpleKind::ProcessName, text.to_owned()));
    }
    normalize_value(CondKind::DomainSuffix, text)
        .map(|domain| (SimpleKind::DomainSuffix, domain))
        .map_err(|_| NOT_RECOGNIZED.to_owned())
}

/// The ready-made list called `name`, ignoring case.
fn ready_list(name: &str) -> Result<&'static ReadyList, String> {
    let name = name.trim();
    READY_LISTS
        .iter()
        .find(|list| list.name.to_lowercase() == name.to_lowercase())
        .ok_or_else(|| format!("Нет готового списка «{name}»"))
}

/// The stored values a value stands for: a ready-made list is its rule-sets.
fn expand(kind: SimpleKind, value: &str) -> Vec<(SimpleKind, String)> {
    match value.strip_prefix("list:") {
        Some(name) => ready_list(name)
            .map(|list| {
                list.rule_sets
                    .iter()
                    .map(|rule_set| (SimpleKind::RuleSet, rule_set.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        None => vec![(kind, value.to_owned())],
    }
}

/// The simple rule type of a family and a bucket.
pub(super) fn type_token(family: Family, bucket: Bucket) -> &'static str {
    match (family, bucket) {
        (Family::Address, Bucket::Proxy) => "simple_address_proxy",
        (Family::Address, Bucket::Direct) => "simple_address_bypass",
        (Family::Address, Bucket::Block) => "simple_address_block",
        (Family::Process, Bucket::Proxy) => "simple_process_name_proxy",
        (Family::Process, Bucket::Direct) => "simple_process_name_bypass",
        (Family::Process, Bucket::Block) => "simple_process_name_block",
    }
}

/// The family and bucket of a simple rule type; `None` for every other type.
pub(super) fn parse_type(rule_type: &str) -> Option<(Family, Bucket)> {
    for family in [Family::Address, Family::Process] {
        for bucket in Bucket::ALL {
            if type_token(family, bucket) == rule_type {
                return Some((family, bucket));
            }
        }
    }
    None
}

/// The name a simple rule gets when it is created, or shows without one.
pub(super) fn default_name(family: Family, bucket: Bucket) -> String {
    format!("{}: {}", family.title(), bucket.title())
}

/// Whether a rule is infrastructure: DNS hijacking, sniffing, resolving, or a rule
/// whose only condition is private addresses or DNS. New simple rules go after the
/// leading run of these.
pub(super) fn is_infrastructure(rule: &RouteRule) -> bool {
    if matches!(rule.action.as_str(), "hijack-dns" | "sniff" | "resolve") {
        return true;
    }
    match present_conditions(rule).as_slice() {
        [only] => {
            only.kind == CondKind::IpIsPrivate
                || (only.kind == CondKind::Protocol && only.values == ["dns"])
        }
        _ => false,
    }
}

impl EditorModel {
    /// The values of a bucket, in the order the screen shows them: sites and
    /// addresses, ready-made lists and rule-sets, then program names.
    pub fn simple_items(&self, bucket: Bucket) -> Vec<SimpleItem> {
        let addresses = self.simple_rules(bucket, Family::Address);
        let processes = self.simple_rules(bucket, Family::Process);

        let mut rule_sets: Vec<String> = Vec::new();
        for rule in &addresses {
            for name in list_values(rule, CondKind::RuleSet) {
                if !rule_sets.contains(name) {
                    rule_sets.push(name.clone());
                }
            }
        }
        let ready: Vec<&ReadyList> = READY_LISTS
            .iter()
            .filter(|list| {
                list.rule_sets
                    .iter()
                    .all(|name| rule_sets.iter().any(|held| held.as_str() == *name))
            })
            .collect();

        let mut items = Vec::new();
        for kind in [
            SimpleKind::Domain,
            SimpleKind::DomainSuffix,
            SimpleKind::DomainKeyword,
            SimpleKind::DomainRegex,
            SimpleKind::IpCidr,
        ] {
            for rule in &addresses {
                for value in list_values(rule, kind.cond()) {
                    push_item(&mut items, kind, shown(kind, value));
                }
            }
        }
        for list in &ready {
            push_item(
                &mut items,
                SimpleKind::RuleSet,
                format!("list:{}", list.name),
            );
        }
        for name in rule_sets {
            let covered = ready
                .iter()
                .any(|list| list.rule_sets.contains(&name.as_str()));
            if !covered {
                push_item(&mut items, SimpleKind::RuleSet, name);
            }
        }
        for rule in &processes {
            for value in list_values(rule, CondKind::ProcessName) {
                push_item(&mut items, SimpleKind::ProcessName, value.clone());
            }
        }
        items
    }

    /// Adds a typed value to a bucket. The value leaves the other buckets. A
    /// ready-made list adds its rule-sets. Nothing changes if the value is there.
    pub fn add_simple(&mut self, bucket: Bucket, input: &str) -> Result<(), String> {
        let (kind, value) = classify(input)?;
        for (kind, value) in expand(kind, &value) {
            self.move_value(bucket, kind, &value);
        }
        Ok(())
    }

    /// Adds a ready-made list to its bucket, as the screen's chips do.
    pub fn add_ready_list(&mut self, name: &str) -> Result<(), String> {
        let list = ready_list(name)?;
        for rule_set in list.rule_sets {
            self.move_value(list.bucket, SimpleKind::RuleSet, rule_set);
        }
        Ok(())
    }

    /// Removes a value from a bucket. A ready-made list (`list:<name>`) removes
    /// all its rule-sets. A simple rule left with no values is deleted.
    pub fn remove_simple(&mut self, bucket: Bucket, kind: SimpleKind, value: &str) {
        let value = match kind {
            SimpleKind::Domain | SimpleKind::DomainSuffix => {
                normalize_host(value).unwrap_or_else(|| value.to_owned())
            }
            _ => value.to_owned(),
        };
        for (kind, value) in expand(kind, &value) {
            self.remove_value_from(bucket, kind, &value);
        }
    }

    /// The simple rules of a family and a bucket, in list order.
    fn simple_rules(&self, bucket: Bucket, family: Family) -> Vec<&RouteRule> {
        self.share
            .rules
            .iter()
            .filter(|rule| parse_type(&rule.r#type) == Some((family, bucket)))
            .collect()
    }

    /// Puts one stored value into a bucket, taking it out of the others first.
    fn move_value(&mut self, bucket: Bucket, kind: SimpleKind, value: &str) {
        for other in Bucket::ALL {
            if other != bucket {
                self.remove_value_from(other, kind, value);
            }
        }
        self.insert_value(bucket, kind, value);
    }

    /// Adds a stored value to a bucket's rule, which is created if there is none.
    fn insert_value(&mut self, bucket: Bucket, kind: SimpleKind, value: &str) {
        let family = kind.family();
        let cond = kind.cond();
        let held = self
            .simple_rules(bucket, family)
            .iter()
            .any(|rule| list_values(rule, cond).iter().any(|item| item == value));
        if held {
            return;
        }
        let index = match self
            .share
            .rules
            .iter()
            .position(|rule| parse_type(&rule.r#type) == Some((family, bucket)))
        {
            Some(index) => index,
            None => {
                let index = self.simple_insert_index(bucket);
                let name = default_name(family, bucket);
                let rule = new_rule(
                    &name,
                    type_token(family, bucket),
                    bucket.action(),
                    bucket.outbound(),
                );
                self.share.rules.insert(index, rule);
                index
            }
        };
        if let Some(field) = list_field_mut(&mut self.share.rules[index], cond) {
            field.get_or_insert_with(Vec::new).push(value.to_owned());
        }
    }

    /// Where a new simple rule of `bucket` goes. It comes after the existing
    /// simple rules of the same or an earlier rank, or before the first one of a
    /// later rank. Without simple rules it goes after the leading infrastructure
    /// rules (see [`is_infrastructure`]), so it lands before every other rule.
    /// Existing rules keep their places.
    fn simple_insert_index(&self, bucket: Bucket) -> usize {
        let rank = bucket.rank();
        let managed: Vec<(usize, u8)> = self
            .share
            .rules
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| {
                parse_type(&rule.r#type).map(|(_, held)| (index, held.rank()))
            })
            .collect();
        if let Some((index, _)) = managed.iter().rev().find(|(_, held)| *held <= rank) {
            return index + 1;
        }
        if let Some((index, _)) = managed.iter().find(|(_, held)| *held > rank) {
            return *index;
        }
        self.share
            .rules
            .iter()
            .take_while(|rule| is_infrastructure(rule))
            .count()
    }

    /// Removes a stored value from a bucket's rules. A rule left with no
    /// conditions is deleted, but only if this removal emptied it.
    fn remove_value_from(&mut self, bucket: Bucket, kind: SimpleKind, value: &str) {
        let family = kind.family();
        let cond = kind.cond();
        self.share.rules.retain_mut(|rule| {
            if parse_type(&rule.r#type) != Some((family, bucket)) {
                return true;
            }
            let Some(field) = list_field_mut(rule, cond) else {
                return true;
            };
            let Some(values) = field.as_mut() else {
                return true;
            };
            let before = values.len();
            values.retain(|item| item.as_str() != value);
            if values.len() == before {
                return true;
            }
            if values.is_empty() {
                *field = None;
            }
            has_conditions(rule)
        });
    }
}

/// The form a stored value takes on the screen: Unicode for domains.
fn shown(kind: SimpleKind, stored: &str) -> String {
    match kind {
        SimpleKind::Domain | SimpleKind::DomainSuffix => domain_for_display(stored),
        _ => stored.to_owned(),
    }
}

/// Adds a chip unless the bucket already shows it.
fn push_item(items: &mut Vec<SimpleItem>, kind: SimpleKind, value: String) {
    let item = SimpleItem {
        kind,
        value,
        label: kind.label(),
    };
    if !items.contains(&item) {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::model_of;
    use super::*;

    const EMPTY: &str = r#"{"name": "T", "rules": []}"#;

    fn types(model: &EditorModel) -> Vec<String> {
        model
            .to_share()
            .rules
            .into_iter()
            .map(|rule| rule.r#type)
            .collect()
    }

    fn item(kind: SimpleKind, value: &str) -> SimpleItem {
        SimpleItem {
            kind,
            value: value.to_owned(),
            label: kind.label(),
        }
    }

    #[test]
    fn classify_recognizes_each_form() {
        let cases = [
            ("192.168.0.0/16", SimpleKind::IpCidr, "192.168.0.0/16"),
            ("10.1.2.3", SimpleKind::IpCidr, "10.1.2.3/32"),
            ("2001:db8::/32", SimpleKind::IpCidr, "2001:db8::/32"),
            ("geosite-ru", SimpleKind::RuleSet, "geosite-ru"),
            ("GeoIP-RU", SimpleKind::RuleSet, "geoip-ru"),
            ("list:Реклама", SimpleKind::RuleSet, "list:Реклама"),
            ("list:реклама", SimpleKind::RuleSet, "list:Реклама"),
            ("telegram", SimpleKind::ProcessName, "telegram"),
            ("steam.exe", SimpleKind::ProcessName, "steam.exe"),
            ("YouTube.com", SimpleKind::DomainSuffix, "youtube.com"),
            ("*.Example.com.", SimpleKind::DomainSuffix, "example.com"),
            (
                "https://www.example.com/path",
                SimpleKind::DomainSuffix,
                "www.example.com",
            ),
            ("full:Example.com", SimpleKind::Domain, "example.com"),
            ("keyword:Ads", SimpleKind::DomainKeyword, "ads"),
            (
                "regex:^ad[0-9]+\\.",
                SimpleKind::DomainRegex,
                "^ad[0-9]+\\.",
            ),
        ];
        for (input, kind, value) in cases {
            assert_eq!(classify(input), Ok((kind, value.to_owned())), "{input:?}");
        }
    }

    #[test]
    fn classify_rejects_garbage_with_the_russian_message() {
        for input in [
            "",
            "   ",
            "!!!",
            "1.2.3",
            "a b",
            "example.com:443",
            "ab..cd",
            "-x.com",
            "list:Нет",
        ] {
            let result = classify(input);
            assert!(result.is_err(), "{input:?} gave {result:?}");
        }
        assert_eq!(
            classify("!!!"),
            Err("Не похоже на сайт, IP или программу".to_owned())
        );
        assert!(classify("full:").is_err());
        assert!(classify("keyword:").is_err());
    }

    #[test]
    fn values_go_to_their_rules_and_show_in_order() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Proxy, "YouTube.com").unwrap();
        model.add_simple(Bucket::Proxy, "youtube.com").unwrap();
        model.add_simple(Bucket::Proxy, "telegram").unwrap();
        model.add_simple(Bucket::Direct, "192.168.0.0/16").unwrap();
        model.add_simple(Bucket::Block, "doubleclick.net").unwrap();
        assert_eq!(
            model.simple_items(Bucket::Proxy),
            vec![
                item(SimpleKind::DomainSuffix, "youtube.com"),
                item(SimpleKind::ProcessName, "telegram")
            ]
        );
        assert_eq!(
            model.simple_items(Bucket::Direct),
            vec![item(SimpleKind::IpCidr, "192.168.0.0/16")]
        );
        assert_eq!(
            model.simple_items(Bucket::Block),
            vec![item(SimpleKind::DomainSuffix, "doubleclick.net")]
        );
    }

    #[test]
    fn new_simple_rules_go_after_the_infrastructure_run_in_block_direct_proxy_order() {
        // Both rules are infrastructure (DNS hijack, private addresses), so the
        // simple rules land after them.
        let mut model = model_of(
            r#"{"name": "T", "rules": [
                {"type": "custom", "action": "hijack-dns", "protocol": "dns"},
                {"type": "custom", "action": "route", "outbound": "direct", "ip_is_private": true}
            ]}"#,
        );
        model.add_simple(Bucket::Proxy, "a.com").unwrap();
        assert_eq!(types(&model), ["custom", "custom", "simple_address_proxy"]);
        model.add_simple(Bucket::Direct, "b.com").unwrap();
        assert_eq!(
            types(&model),
            [
                "custom",
                "custom",
                "simple_address_bypass",
                "simple_address_proxy"
            ]
        );
        model.add_simple(Bucket::Block, "c.com").unwrap();
        assert_eq!(
            types(&model),
            [
                "custom",
                "custom",
                "simple_address_block",
                "simple_address_bypass",
                "simple_address_proxy"
            ]
        );
        let shared = model.to_share();
        assert_eq!(
            (
                shared.rules[4].action.as_str(),
                shared.rules[4].outbound.clone()
            ),
            ("route", Some(Outbound::Proxy))
        );
        assert_eq!(
            (
                shared.rules[3].action.as_str(),
                shared.rules[3].outbound.clone()
            ),
            ("route", Some(Outbound::Direct))
        );
        assert_eq!(
            (
                shared.rules[2].action.as_str(),
                shared.rules[2].outbound.clone()
            ),
            ("reject", None)
        );
    }

    #[test]
    fn bypass_russia_gets_simple_rules_after_its_infrastructure_rules() {
        let mut model = EditorModel::from_share(&super::super::fixtures::bypass_russia(), false);
        model.add_simple(Bucket::Proxy, "a.com").unwrap();
        // hijack-dns and private-IP rules stay on top, the rule-set rule goes last.
        assert_eq!(
            types(&model),
            ["custom", "custom", "simple_address_proxy", "custom"]
        );
        assert_eq!(model.to_share().rules[3].name, "imported rule #3");
        model.add_simple(Bucket::Block, "b.com").unwrap();
        model.add_simple(Bucket::Direct, "c.com").unwrap();
        assert_eq!(
            types(&model),
            [
                "custom",
                "custom",
                "simple_address_block",
                "simple_address_bypass",
                "simple_address_proxy",
                "custom"
            ]
        );
    }

    #[test]
    fn a_profile_without_infrastructure_gets_simple_rules_first() {
        let mut model = model_of(
            r#"{"name": "T", "rules": [
                {"type": "custom", "action": "route", "outbound": "direct", "rule_set": ["geosite-ru"]}
            ]}"#,
        );
        model.add_simple(Bucket::Proxy, "a.com").unwrap();
        assert_eq!(types(&model), ["simple_address_proxy", "custom"]);
    }

    #[test]
    fn infrastructure_rules_are_recognized() {
        let rule = |json: &str| -> RouteRule { serde_json::from_str(json).expect("parses") };
        assert!(is_infrastructure(&rule(r#"{"action": "sniff"}"#)));
        assert!(is_infrastructure(&rule(
            r#"{"action": "resolve", "domain": ["a.com"]}"#
        )));
        assert!(is_infrastructure(&rule(
            r#"{"action": "route", "protocol": "dns"}"#
        )));
        assert!(is_infrastructure(&rule(
            r#"{"action": "route", "outbound": "direct", "ip_is_private": false}"#
        )));
        assert!(!is_infrastructure(&rule(
            r#"{"action": "route", "protocol": "dns", "network": "udp"}"#
        )));
        assert!(!is_infrastructure(&rule(
            r#"{"action": "route", "outbound": "direct", "rule_set": ["geosite-ru"]}"#
        )));
        assert!(!is_infrastructure(&rule(
            r#"{"action": "route", "domain": ["a.com"]}"#
        )));
    }

    #[test]
    fn unicode_domains_are_stored_as_punycode_and_shown_in_unicode() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Direct, "кремль.рф").unwrap();
        model
            .add_simple(Bucket::Direct, "Пример.Испытание")
            .unwrap();
        assert_eq!(types(&model), ["simple_address_bypass"]);
        assert_eq!(
            model.to_share().rules[0].domain_suffix,
            Some(vec![
                "xn--e1ajeds9e.xn--p1ai".to_owned(),
                "xn--e1afmkfd.xn--80akhbyknj4f".to_owned()
            ])
        );
        assert_eq!(
            model.simple_items(Bucket::Direct),
            vec![
                item(SimpleKind::DomainSuffix, "кремль.рф"),
                item(SimpleKind::DomainSuffix, "пример.испытание")
            ]
        );
        model.remove_simple(Bucket::Direct, SimpleKind::DomainSuffix, "кремль.рф");
        assert_eq!(model.simple_items(Bucket::Direct).len(), 1);
        model
            .add_simple(Bucket::Block, "xn--e1ajeds9e.xn--p1ai")
            .unwrap();
        assert_eq!(
            model.simple_items(Bucket::Block),
            vec![item(SimpleKind::DomainSuffix, "кремль.рф")]
        );
    }

    #[test]
    fn existing_simple_rules_keep_their_order() {
        let mut model = model_of(
            r#"{"name": "T", "rules": [
                {"type": "custom", "action": "route", "outbound": "proxy", "domain": ["x.com"]},
                {"type": "simple_address_proxy", "action": "route", "outbound": "proxy", "domain_suffix": ["a.com"]},
                {"type": "custom", "action": "hijack-dns", "protocol": "dns"}
            ]}"#,
        );
        model.add_simple(Bucket::Block, "b.com").unwrap();
        model.add_simple(Bucket::Direct, "c.com").unwrap();
        assert_eq!(
            types(&model),
            [
                "custom",
                "simple_address_block",
                "simple_address_bypass",
                "simple_address_proxy",
                "custom"
            ]
        );
        let shared = model.to_share();
        assert_eq!(
            shared.rules[0].domain.as_deref(),
            Some(&["x.com".to_owned()][..])
        );
        assert_eq!(shared.rules[4].protocol.as_deref(), Some("dns"));
    }

    #[test]
    fn process_names_get_their_own_rule_per_bucket() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Direct, "steam.exe").unwrap();
        assert_eq!(types(&model), ["simple_process_name_bypass"]);
        assert_eq!(
            model.simple_items(Bucket::Direct),
            vec![item(SimpleKind::ProcessName, "steam.exe")]
        );
        assert_eq!(model.to_share().rules[0].name, "Программы: Напрямую");
    }

    #[test]
    fn moving_a_value_takes_it_out_of_the_old_bucket() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Proxy, "gosuslugi.ru").unwrap();
        model.add_simple(Bucket::Direct, "gosuslugi.ru").unwrap();
        assert!(model.simple_items(Bucket::Proxy).is_empty());
        assert_eq!(
            model.simple_items(Bucket::Direct),
            vec![item(SimpleKind::DomainSuffix, "gosuslugi.ru")]
        );
        assert_eq!(types(&model), ["simple_address_bypass"]);
    }

    #[test]
    fn remove_simple_drops_the_rule_when_it_is_empty() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Proxy, "a.com").unwrap();
        model.add_simple(Bucket::Proxy, "telegram").unwrap();
        model.add_simple(Bucket::Block, "b.com").unwrap();
        model.remove_simple(Bucket::Proxy, SimpleKind::DomainSuffix, "a.com");
        assert_eq!(
            types(&model),
            ["simple_address_block", "simple_process_name_proxy"]
        );
        model.remove_simple(Bucket::Proxy, SimpleKind::ProcessName, "telegram");
        assert_eq!(types(&model), ["simple_address_block"]);
        model.remove_simple(Bucket::Block, SimpleKind::DomainSuffix, "missing.com");
        assert_eq!(types(&model), ["simple_address_block"]);
    }

    #[test]
    fn removal_keeps_rules_that_were_already_empty() {
        let mut model = model_of(
            r#"{"name": "T", "rules": [{"type": "simple_address_proxy", "action": "route", "outbound": "proxy"}]}"#,
        );
        model.remove_simple(Bucket::Proxy, SimpleKind::DomainSuffix, "a.com");
        assert_eq!(types(&model), ["simple_address_proxy"]);
    }

    #[test]
    fn ready_list_adds_its_rule_sets_and_shows_as_one_chip() {
        let mut model = model_of(EMPTY);
        model.add_ready_list("Реклама").unwrap();
        assert_eq!(
            model.simple_items(Bucket::Block),
            vec![item(SimpleKind::RuleSet, "list:Реклама")]
        );
        assert_eq!(types(&model), ["simple_address_block"]);
        assert_eq!(
            model.to_share().rules[0].rule_set,
            Some(vec!["geosite-category-ads-all".to_owned()])
        );
        // Throne's block rule is a reject with no outbound.
        assert_eq!(model.to_share().rules[0].action, "reject");
        assert_eq!(model.to_share().rules[0].outbound, None);
    }

    #[test]
    fn ready_list_is_removed_as_one_chip() {
        let mut model = model_of(EMPTY);
        model.add_ready_list("Заблокированное в РФ").unwrap();
        model.add_ready_list("Стриминг").unwrap();
        model.remove_simple(
            Bucket::Proxy,
            SimpleKind::RuleSet,
            "list:Заблокированное в РФ",
        );
        assert_eq!(
            model.simple_items(Bucket::Proxy),
            vec![item(SimpleKind::RuleSet, "list:Стриминг")]
        );
        model.remove_simple(Bucket::Proxy, SimpleKind::RuleSet, "list:Стриминг");
        assert!(types(&model).is_empty());
    }

    #[test]
    fn partly_moved_ready_list_shows_its_rest_as_rule_sets() {
        let mut model = model_of(EMPTY);
        model.add_ready_list("Заблокированное в РФ").unwrap();
        model
            .add_simple(Bucket::Direct, "geosite-ru-blocked")
            .unwrap();
        assert_eq!(
            model.simple_items(Bucket::Proxy),
            vec![item(SimpleKind::RuleSet, "geoip-ru-blocked")]
        );
        assert_eq!(
            model.simple_items(Bucket::Direct),
            vec![item(SimpleKind::RuleSet, "geosite-ru-blocked")]
        );
    }

    #[test]
    fn unknown_ready_list_is_an_error() {
        let mut model = model_of(EMPTY);
        assert!(model.add_simple(Bucket::Proxy, "list:Нет такого").is_err());
        assert!(model.add_ready_list("Нет такого").is_err());
        assert!(types(&model).is_empty());
    }

    #[test]
    fn available_ready_lists_hide_unknown_rule_sets() {
        assert_eq!(available_ready_lists(None).len(), 4);
        let known = vec![
            "geosite-ru-blocked".to_owned(),
            "geoip-ru-blocked".to_owned(),
        ];
        let names: Vec<&str> = available_ready_lists(Some(&known))
            .iter()
            .map(|list| list.name)
            .collect();
        assert_eq!(names, ["Заблокированное в РФ"]);
        assert!(available_ready_lists(Some(&[])).is_empty());
    }

    #[test]
    fn dedupes_within_a_bucket() {
        let mut model = model_of(EMPTY);
        model.add_simple(Bucket::Proxy, "a.com").unwrap();
        model.add_simple(Bucket::Proxy, "A.COM").unwrap();
        assert_eq!(model.simple_items(Bucket::Proxy).len(), 1);
        assert_eq!(types(&model).len(), 1);
    }
}
