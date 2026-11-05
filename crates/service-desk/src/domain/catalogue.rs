//! The Portal Catalogue (FR-PRT-002): the Categories a Requester can choose from when
//! raising a ticket on the Self-Service Portal.

use std::collections::HashMap;

use uuid::Uuid;

use super::{Category, Lookup};

/// The Portal Catalogue: active, portal-visible Categories whose ancestors are all
/// active and portal-visible too, in tree order (each parent before its children,
/// siblings by sort order, then name).
#[must_use]
pub fn portal_catalogue(categories: &[Category]) -> Vec<&Category> {
    let by_id: HashMap<Uuid, &Category> = categories.iter().map(|c| (c.id(), c)).collect();
    let offered = |c: &Category| -> bool {
        let mut current = Some(c);
        let mut steps = 0;
        while let Some(node) = current {
            if !node.is_active() || !node.portal_visible() || steps > 3 {
                return false;
            }
            steps += 1;
            current = match node.parent_id() {
                None => None,
                Some(p) => match by_id.get(&p) {
                    Some(parent) => Some(*parent),
                    None => return false,
                },
            };
        }
        true
    };
    let mut children: HashMap<Option<Uuid>, Vec<&Category>> = HashMap::new();
    for c in categories.iter().filter(|c| offered(c)) {
        children.entry(c.parent_id()).or_default().push(c);
    }
    for list in children.values_mut() {
        list.sort_by(|a, b| {
            (a.sort_order(), a.name().to_lowercase())
                .cmp(&(b.sort_order(), b.name().to_lowercase()))
        });
    }
    let mut out = Vec::new();
    let mut stack: Vec<&Category> = children.get(&None).cloned().unwrap_or_default();
    stack.reverse();
    while let Some(c) = stack.pop() {
        out.push(c);
        if let Some(kids) = children.get(&Some(c.id())) {
            stack.extend(kids.iter().rev());
        }
    }
    out
}

/// The Ticket Type of a ticket raised on the Portal under `category`: none when the
/// Category allows every Type, otherwise the first active allowed Type in display
/// order (`types` is the Ticket Type reference data).
#[must_use]
pub fn portal_ticket_type(category: &Category, types: &[Lookup]) -> Option<Uuid> {
    if category.type_ids().is_empty() {
        return None;
    }
    types
        .iter()
        .filter(|t| t.is_active() && category.type_ids().contains(&t.id()))
        .min_by_key(|t| (t.sort_order(), t.name().to_lowercase()))
        .map(Lookup::id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CategoryInput, LookupFlags, LookupKind};
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeSet;

    fn cat(parent: Option<&Category>, name: &str, visible: bool, order: i32) -> Category {
        Category::create(
            parent,
            CategoryInput {
                name: name.into(),
                portal_visible: visible,
                type_ids: BTreeSet::new(),
                sort_order: order,
                is_active: true,
            },
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn catalogue_hides_categories_under_hidden_or_inactive_parents_and_keeps_tree_order() {
        let hardware = cat(None, "Hardware", true, 2);
        let laptop = cat(Some(&hardware), "Laptop", true, 1);
        let battery = cat(Some(&laptop), "Battery", true, 1);
        let software = cat(None, "Software", true, 1);
        let internal = cat(None, "Internal", false, 0);
        let under_internal = cat(Some(&internal), "Visible child", true, 0);
        let mut retired = cat(Some(&software), "Retired", true, 0);
        retired
            .update(
                CategoryInput {
                    name: "Retired".into(),
                    portal_visible: true,
                    type_ids: BTreeSet::new(),
                    sort_order: 0,
                    is_active: false,
                },
                Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
            )
            .unwrap();
        let all = vec![
            battery.clone(),
            laptop.clone(),
            hardware.clone(),
            under_internal,
            internal,
            retired,
            software.clone(),
        ];
        let names: Vec<&str> = portal_catalogue(&all).iter().map(|c| c.name()).collect();
        assert_eq!(names, ["Software", "Hardware", "Laptop", "Battery"]);
    }

    #[test]
    fn portal_tickets_take_the_first_allowed_active_type() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let ty = |name: &str, order: i32| {
            Lookup::create(LookupKind::Type, name, LookupFlags::default(), order, now).unwrap()
        };
        let (incident, request, change) = (ty("Incident", 1), ty("Request", 2), ty("Change", 0));
        let mut c = cat(None, "Access", true, 0);
        assert_eq!(
            portal_ticket_type(&c, std::slice::from_ref(&incident)),
            None
        );
        c.update(
            CategoryInput {
                name: "Access".into(),
                portal_visible: true,
                type_ids: BTreeSet::from([request.id(), incident.id()]),
                sort_order: 0,
                is_active: true,
            },
            now,
        )
        .unwrap();
        assert_eq!(
            portal_ticket_type(&c, &[request.clone(), incident.clone(), change]),
            Some(incident.id())
        );
    }
}
