//! The Servers page: group tabs, the server grid and the rules behind them.
//!
//! Everything here is pure. The engine collects Throne's groups and profiles,
//! [`servers_view`] turns them into what the page shows, and the Slint side only
//! lays the result out.

use throne_api::types::{Group, Job, Profile};

use crate::view::{ago_text, proto_tag, server_row, RouteNote, ServerRow};

/// Group id of the "Все" tab: every profile, whatever its group.
pub const ALL_GROUPS: i64 = -1;

/// Label of the "Все" tab.
const ALL_LABEL: &str = "Все";

/// A request of the page that is in flight, shown on its control until it finishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServersBusy {
    /// Updating the subscription group with this id.
    Update(i64),
    /// Deleting the selected group.
    DeleteGroup,
    /// Adding what the "Ссылки" box holds.
    Import,
    /// Adding a subscription.
    AddSubscription,
}

/// One tab of the group bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTab {
    /// Group id, or [`ALL_GROUPS`] for "Все".
    pub id: i64,
    pub label: String,
    pub count: u64,
    /// Shown with a refresh mark: the group is a subscription.
    pub subscription: bool,
}

/// The card of the selected subscription group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscriptionCard {
    pub name: String,
    /// "обновлено 2 ч назад", or "ещё не обновлялась".
    pub meta: String,
    /// "6 серверов".
    pub count: String,
}

/// The slim row of the selected plain group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainGroup {
    pub name: String,
    /// "1 сервер".
    pub count: String,
}

/// Everything the Servers page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServersView {
    pub groups: Vec<GroupTab>,
    /// The tab that is on, after resolving an id that no longer exists.
    pub selected: i64,
    pub subscription: Option<SubscriptionCard>,
    pub plain: Option<PlainGroup>,
    /// The selected tab is a group and another group remains to fall back to.
    pub can_delete: bool,
    pub cards: Vec<ServerRow>,
    pub by_ping: bool,
    /// Shown instead of the grid when it has no cards.
    pub empty_text: String,
    /// A test is running: `(tested, total)`.
    pub test: Option<(usize, usize)>,
    pub busy: Option<ServersBusy>,
    /// The group the "Ссылки" import goes into, or [`ALL_GROUPS`] for Throne's default.
    pub import_target: i64,
    pub note: RouteNote,
}

/// Inputs of [`servers_view`], gathered by the engine.
pub struct ServersInput<'a> {
    pub groups: &'a [Group],
    pub profiles: &'a [Profile],
    /// The tab remembered in the preferences; may name a group that is gone.
    pub selected: i64,
    pub query: &'a str,
    pub by_ping: bool,
    pub test: Option<(usize, usize)>,
    pub busy: Option<ServersBusy>,
    pub note: RouteNote,
    /// Epoch seconds, for "обновлено N назад".
    pub now: i64,
}

/// Which profiles a latency test covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestSelection {
    /// Every profile of this group, in Throne's order.
    Group(i64),
    /// These profiles, in this order.
    Profiles(Vec<i64>),
}

/// Computes the page from the engine's state.
pub fn servers_view(input: &ServersInput) -> ServersView {
    let selected = resolve_selected(input.groups, input.selected);
    let current = input.groups.iter().find(|group| group.id == selected);
    let cards = visible_cards(input.profiles, selected, input.query, input.by_ping);
    let empty_text = match (cards.is_empty(), input.query.trim()) {
        (false, _) => String::new(),
        (true, "") if current.is_some() => "В этой группе пока нет серверов".to_owned(),
        (true, "") => "Пока нет серверов — добавь ссылку или подписку".to_owned(),
        (true, query) => format!("Ничего не нашлось по «{query}»"),
    };
    ServersView {
        groups: group_tabs(input.groups, input.profiles.len() as u64),
        selected,
        subscription: current
            .filter(|group| group.subscription)
            .map(|group| SubscriptionCard {
                name: group.name.clone(),
                meta: subscription_meta(group, input.now),
                count: servers_count_text(group.profile_count),
            }),
        plain: current
            .filter(|group| !group.subscription)
            .map(|group| PlainGroup {
                name: group.name.clone(),
                count: servers_count_text(group.profile_count),
            }),
        can_delete: current.is_some() && input.groups.len() > 1,
        cards,
        by_ping: input.by_ping,
        empty_text,
        test: input.test,
        busy: input.busy.clone(),
        import_target: import_target(input.groups, selected),
        note: input.note.clone(),
    }
}

/// The tab to show: `selected` when that group exists, "Все" otherwise.
pub fn resolve_selected(groups: &[Group], selected: i64) -> i64 {
    if selected != ALL_GROUPS && groups.iter().any(|group| group.id == selected) {
        selected
    } else {
        ALL_GROUPS
    }
}

/// "Все" with the number of all profiles, then every group in Throne's tab order.
pub fn group_tabs(groups: &[Group], total: u64) -> Vec<GroupTab> {
    let mut tabs = vec![GroupTab {
        id: ALL_GROUPS,
        label: ALL_LABEL.to_owned(),
        count: total,
        subscription: false,
    }];
    tabs.extend(groups.iter().map(|group| GroupTab {
        id: group.id,
        label: group.name.clone(),
        count: group.profile_count,
        subscription: group.subscription,
    }));
    tabs
}

/// The group a plain import goes into: the selected group when it is a plain one,
/// [`ALL_GROUPS`] (Throne's default group) otherwise.
pub fn import_target(groups: &[Group], selected: i64) -> i64 {
    groups
        .iter()
        .find(|group| group.id == selected && !group.subscription)
        .map_or(ALL_GROUPS, |group| group.id)
}

/// Whether a profile belongs to the selected tab.
pub fn in_group(profile: &Profile, selected: i64) -> bool {
    selected == ALL_GROUPS || profile.group == selected
}

/// Whether a profile matches the search text: a case-insensitive part of its name,
/// address, type, country or protocol tag. An empty search matches everything.
pub fn matches_query(profile: &Profile, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    [
        profile.name.as_str(),
        profile.address.as_str(),
        profile.r#type.as_str(),
        profile.country.as_str(),
    ]
    .iter()
    .any(|field| field.to_lowercase().contains(&needle))
        || proto_tag(&profile.r#type).to_lowercase().contains(&needle)
}

/// Orders cards: by ping when asked (fastest first, untested and failed last), else
/// as Throne lists them. The sort is stable, so equal pings keep the group order.
pub fn order_cards(rows: &mut [ServerRow], by_ping: bool) {
    if by_ping {
        rows.sort_by_key(|row| (row.latency <= 0, row.latency.max(0)));
    }
}

/// The cards of the selected tab that match the search, in display order.
pub fn visible_cards(
    profiles: &[Profile],
    selected: i64,
    query: &str,
    by_ping: bool,
) -> Vec<ServerRow> {
    let mut rows: Vec<ServerRow> = profiles
        .iter()
        .filter(|profile| in_group(profile, selected) && matches_query(profile, query))
        .map(server_row)
        .collect();
    order_cards(&mut rows, by_ping);
    rows
}

/// What "Тест пинга" tests on the selected tab. `None` when there is nothing to test.
///
/// Auto selectors are never tested, so they are left out of "Все".
pub fn test_selection(profiles: &[Profile], selected: i64) -> Option<TestSelection> {
    if selected != ALL_GROUPS {
        return profiles
            .iter()
            .any(|profile| profile.group == selected)
            .then_some(TestSelection::Group(selected));
    }
    let ids: Vec<i64> = profiles
        .iter()
        .filter(|profile| profile.r#type != "autoselector")
        .map(|profile| profile.id)
        .collect();
    (!ids.is_empty()).then_some(TestSelection::Profiles(ids))
}

/// The progress text of a running test.
pub fn test_progress_text(tested: usize, total: usize) -> String {
    format!("Тестим… {tested}/{total}")
}

/// How many of a test's profiles have been measured since the test started.
pub fn tested_count(profiles: &[Profile], job: &Job) -> usize {
    profiles
        .iter()
        .filter(|profile| {
            job.profile_ids.contains(&profile.id) && profile.latency_at >= job.started_at
        })
        .count()
}

/// Replaces the listed profiles that `results` has, by id. Others are left alone.
pub fn merge_results(profiles: &mut [Profile], results: &[Profile]) {
    for result in results {
        if let Some(profile) = profiles.iter_mut().find(|p| p.id == result.id) {
            *profile = result.clone();
        }
    }
}

/// "обновлено 2 ч назад", or "ещё не обновлялась" for a subscription never updated.
pub fn subscription_meta(group: &Group, now: i64) -> String {
    if group.updated_at > 0 {
        format!("обновлено {}", ago_text(now, group.updated_at))
    } else {
        "ещё не обновлялась".to_owned()
    }
}

/// Russian count of servers: "1 сервер", "3 сервера", "7 серверов".
pub fn servers_count_text(count: u64) -> String {
    let word = match (count % 10, count % 100) {
        (1, n) if n != 11 => "сервер",
        (2..=4, n) if !(12..=14).contains(&n) => "сервера",
        _ => "серверов",
    };
    format!("{count} {word}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: i64, name: &str, subscription: bool, profile_count: u64) -> Group {
        Group {
            id,
            name: name.to_owned(),
            subscription,
            archived: false,
            profile_count,
            updated_at: 0,
        }
    }

    fn profile(id: i64, group: i64, latency: i64) -> Profile {
        Profile {
            id,
            group,
            name: format!("server {id}"),
            r#type: "vless".to_owned(),
            latency_ms: latency,
            ..Profile::default()
        }
    }

    fn input<'a>(
        groups: &'a [Group],
        profiles: &'a [Profile],
        selected: i64,
        query: &'a str,
    ) -> ServersInput<'a> {
        ServersInput {
            groups,
            profiles,
            selected,
            query,
            by_ping: false,
            test: None,
            busy: None,
            note: RouteNote::default(),
            now: 1_000_000,
        }
    }

    #[test]
    fn unknown_or_missing_group_falls_back_to_all() {
        let groups = [group(3, "Подписка", true, 1)];
        assert_eq!(resolve_selected(&groups, 3), 3);
        assert_eq!(resolve_selected(&groups, 9), ALL_GROUPS);
        assert_eq!(resolve_selected(&groups, ALL_GROUPS), ALL_GROUPS);
        assert_eq!(resolve_selected(&[], 3), ALL_GROUPS);
    }

    #[test]
    fn tabs_start_with_all_and_keep_the_group_order() {
        let groups = [group(5, "Ручные", false, 2), group(3, "Подписка", true, 7)];
        let tabs = group_tabs(&groups, 9);
        let labels: Vec<(&str, u64, bool)> = tabs
            .iter()
            .map(|tab| (tab.label.as_str(), tab.count, tab.subscription))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("Все", 9, false),
                ("Ручные", 2, false),
                ("Подписка", 7, true)
            ]
        );
        assert_eq!(tabs[0].id, ALL_GROUPS);
        assert_eq!(tabs[2].id, 3);
    }

    #[test]
    fn search_ignores_case_and_spaces_around_the_text() {
        let mut p = profile(1, 1, 0);
        p.name = "Амстердам 2".to_owned();
        p.address = "NL1.example.net:443".to_owned();
        p.country = "nl".to_owned();
        p.r#type = "hysteria2".to_owned();

        assert!(matches_query(&p, ""));
        assert!(matches_query(&p, "   "));
        assert!(matches_query(&p, "амстер"));
        assert!(matches_query(&p, " NL1.EXAMPLE "));
        assert!(matches_query(&p, "NL"));
        assert!(matches_query(&p, "hy2"));
        assert!(matches_query(&p, "HYSTERIA"));
        assert!(!matches_query(&p, "токио"));
    }

    #[test]
    fn cards_follow_the_selected_group_and_the_search() {
        let profiles = [profile(1, 1, 0), profile(2, 2, 0), profile(3, 1, 0)];
        let ids = |rows: Vec<ServerRow>| rows.iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(
            ids(visible_cards(&profiles, ALL_GROUPS, "", false)),
            [1, 2, 3]
        );
        assert_eq!(ids(visible_cards(&profiles, 1, "", false)), [1, 3]);
        assert_eq!(ids(visible_cards(&profiles, 1, "server 3", false)), [3]);
    }

    #[test]
    fn by_ping_puts_fast_first_and_untested_or_failed_last() {
        let profiles = [
            profile(1, 1, 0),
            profile(2, 1, 120),
            profile(3, 1, -1),
            profile(4, 1, 40),
            profile(5, 1, 0),
        ];
        let ids: Vec<i64> = visible_cards(&profiles, ALL_GROUPS, "", true)
            .iter()
            .map(|r| r.id)
            .collect();
        // Untested (1, 5) and failed (3) keep their group order at the end.
        assert_eq!(ids, [4, 2, 1, 3, 5]);
        let ids: Vec<i64> = visible_cards(&profiles, ALL_GROUPS, "", false)
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, [1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_covers_the_group_or_every_profile_but_auto_selectors() {
        let mut auto = profile(4, 1, 0);
        auto.r#type = "autoselector".to_owned();
        let profiles = [profile(1, 1, 0), profile(2, 2, 0), auto];
        assert_eq!(test_selection(&profiles, 2), Some(TestSelection::Group(2)));
        assert_eq!(
            test_selection(&profiles, ALL_GROUPS),
            Some(TestSelection::Profiles(vec![1, 2]))
        );
        // An empty group has nothing to test, and so has an empty list.
        assert_eq!(test_selection(&profiles, 7), None);
        assert_eq!(test_selection(&[], ALL_GROUPS), None);
    }

    #[test]
    fn progress_counts_profiles_measured_after_the_start() {
        let job = Job {
            id: 1,
            kind: throne_api::types::JobKind::Latency,
            state: throne_api::types::JobState::Running,
            profile_ids: vec![1, 2, 3],
            started_at: 100,
            finished_at: None,
            results: None,
        };
        let mut old = profile(1, 1, 50);
        old.latency_at = 99;
        let mut fresh = profile(2, 1, -1);
        fresh.latency_at = 100;
        let mut outside = profile(9, 1, 30);
        outside.latency_at = 200;
        let profiles = [old, fresh, profile(3, 1, 0), outside];
        assert_eq!(tested_count(&profiles, &job), 1);
        assert_eq!(test_progress_text(1, 3), "Тестим… 1/3");
    }

    #[test]
    fn results_replace_the_listed_profiles_by_id() {
        let mut listed = vec![profile(1, 1, 0), profile(2, 1, 0)];
        let mut tested = profile(2, 1, -1);
        tested.latency_at = 500;
        merge_results(&mut listed, &[tested.clone(), profile(9, 1, 30)]);
        assert_eq!(listed[0].latency_ms, 0);
        assert_eq!(listed[1], tested);
        assert_eq!(listed.len(), 2, "unknown ids are not added");
    }

    #[test]
    fn plain_group_gets_a_row_and_subscription_a_card() {
        let mut sub = group(3, "Моя подписка", true, 6);
        sub.updated_at = 1_000_000 - 7_300;
        let groups = [sub, group(4, "Ручные", false, 1)];
        let on_sub = servers_view(&input(&groups, &[], 3, ""));
        let card = on_sub.subscription.expect("subscription card");
        assert_eq!(card.meta, "обновлено 2 ч назад");
        assert_eq!(card.count, "6 серверов");
        assert!(on_sub.plain.is_none());
        assert!(on_sub.can_delete);

        let on_plain = servers_view(&input(&groups, &[], 4, ""));
        assert!(on_plain.subscription.is_none());
        let row = on_plain.plain.expect("plain row");
        assert_eq!(row.name, "Ручные");
        assert_eq!(row.count, "1 сервер");
    }

    #[test]
    fn the_only_group_cannot_be_deleted_and_all_has_no_card() {
        let groups = [group(1, "Ручные", false, 0)];
        let view = servers_view(&input(&groups, &[], ALL_GROUPS, ""));
        assert!(!view.can_delete);
        assert!(view.subscription.is_none() && view.plain.is_none());
        let view = servers_view(&input(&groups, &[], 1, ""));
        assert!(!view.can_delete);
        assert!(view.plain.is_some());
    }

    #[test]
    fn imports_go_to_the_selected_plain_group_only() {
        let groups = [group(1, "Ручные", false, 0), group(2, "Подписка", true, 0)];
        assert_eq!(import_target(&groups, 1), 1);
        assert_eq!(import_target(&groups, 2), ALL_GROUPS);
        assert_eq!(import_target(&groups, ALL_GROUPS), ALL_GROUPS);
    }

    #[test]
    fn subscription_never_updated_says_so() {
        let sub = group(3, "Подписка", true, 0);
        assert_eq!(subscription_meta(&sub, 5), "ещё не обновлялась");
        let mut updated = sub;
        updated.updated_at = 1_000;
        assert_eq!(
            subscription_meta(&updated, 1_000 + 120),
            "обновлено 2 мин назад"
        );
    }

    #[test]
    fn empty_grid_says_why() {
        let groups = [group(1, "Ручные", false, 0)];
        let view = servers_view(&input(&groups, &[], ALL_GROUPS, ""));
        assert_eq!(
            view.empty_text,
            "Пока нет серверов — добавь ссылку или подписку"
        );
        let view = servers_view(&input(&groups, &[], 1, ""));
        assert_eq!(view.empty_text, "В этой группе пока нет серверов");
        let profiles = [profile(1, 1, 0)];
        let view = servers_view(&input(&groups, &profiles, ALL_GROUPS, "токио"));
        assert_eq!(view.empty_text, "Ничего не нашлось по «токио»");
        let view = servers_view(&input(&groups, &profiles, ALL_GROUPS, ""));
        assert!(view.empty_text.is_empty());
    }

    #[test]
    fn server_counts_are_russian() {
        assert_eq!(servers_count_text(0), "0 серверов");
        assert_eq!(servers_count_text(1), "1 сервер");
        assert_eq!(servers_count_text(2), "2 сервера");
        assert_eq!(servers_count_text(11), "11 серверов");
        assert_eq!(servers_count_text(22), "22 сервера");
        assert_eq!(servers_count_text(112), "112 серверов");
    }
}
