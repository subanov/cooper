//! Ticket Type Rules (FR-TKT-011): choose the Ticket Type of a new ticket when nobody
//! chose one. Rules run in order and the first whose conditions all match wins. A type
//! someone chose always wins, and a rule never stops a ticket from being created.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use cooper_kernel::{DepartmentId, TenantId};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::TicketError;

/// How a ticket arrived (fixed set).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Email intake.
    Email,
    /// The Self-Service Portal.
    Portal,
    /// An Analyst in the console, or an integration through the API.
    Analyst,
}

impl Channel {
    /// Stable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::Portal => "portal",
            Self::Analyst => "analyst",
        }
    }
}

impl FromStr for Channel {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "email" => Ok(Self::Email),
            "portal" => Ok(Self::Portal),
            "analyst" => Ok(Self::Analyst),
            _ => Err(TicketError::invalid(
                "channels",
                "must be email, portal or analyst",
            )),
        }
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Conditions of a rule. An empty list is ignored; every non-empty one must match.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TicketTypeRuleConditions {
    /// How the ticket arrived.
    pub channels: Vec<Channel>,
    /// The Mailbox it arrived at.
    pub mailbox_ids: Vec<Uuid>,
    /// Its Origin.
    pub origin_ids: Vec<Uuid>,
    /// Its Department.
    pub department_ids: Vec<DepartmentId>,
    /// Its Company.
    pub company_ids: Vec<TenantId>,
    /// Words or phrases; any one found in the subject (ignoring case) matches.
    pub subject_keywords: Vec<String>,
    /// Requester email domains; a subdomain matches too.
    pub requester_domains: Vec<String>,
}

impl TicketTypeRuleConditions {
    fn validate(mut self) -> Result<Self, TicketError> {
        let clean = |items: Vec<String>, field: &'static str, domain: bool| {
            let mut out: Vec<String> = Vec::new();
            for item in items {
                let mut item = item.trim().to_lowercase();
                if domain {
                    item = item
                        .trim_start_matches('@')
                        .trim_end_matches('.')
                        .to_owned();
                }
                if item.is_empty() {
                    continue;
                }
                if item.chars().count() > 200 {
                    return Err(TicketError::invalid(
                        field,
                        "entries must be at most 200 characters",
                    ));
                }
                if !out.contains(&item) {
                    out.push(item);
                }
            }
            if out.len() > 100 {
                return Err(TicketError::invalid(field, "at most 100 entries"));
            }
            Ok(out)
        };
        self.subject_keywords = clean(self.subject_keywords, "subject_keywords", false)?;
        self.requester_domains = clean(self.requester_domains, "requester_domains", true)?;
        if self == Self::default() {
            return Err(TicketError::invalid(
                "conditions",
                "a rule needs at least one condition",
            ));
        }
        Ok(self)
    }

    fn matches(&self, t: &RuleSubject<'_>) -> bool {
        fn any<T: PartialEq>(list: &[T], value: Option<&T>) -> bool {
            list.is_empty() || value.is_some_and(|v| list.contains(v))
        }
        let subject = t.subject.to_lowercase();
        let domain = t
            .requester_email
            .and_then(|e| e.rsplit_once('@'))
            .map(|(_, d)| d.trim_end_matches('.').to_lowercase());
        any(&self.channels, Some(&t.channel))
            && any(&self.mailbox_ids, t.mailbox_id.as_ref())
            && any(&self.origin_ids, t.origin_id.as_ref())
            && any(&self.department_ids, t.department_id.as_ref())
            && any(&self.company_ids, Some(&t.tenant_id))
            && (self.subject_keywords.is_empty()
                || self
                    .subject_keywords
                    .iter()
                    .any(|k| subject.contains(k.as_str())))
            && (self.requester_domains.is_empty()
                || domain.is_some_and(|d| {
                    self.requester_domains
                        .iter()
                        .any(|r| d == *r || d.ends_with(&format!(".{r}")))
                }))
    }
}

/// What a rule looks at.
#[derive(Debug, Clone, Copy)]
pub struct RuleSubject<'a> {
    /// How the ticket arrived.
    pub channel: Channel,
    /// The Mailbox.
    pub mailbox_id: Option<Uuid>,
    /// The Origin.
    pub origin_id: Option<Uuid>,
    /// The Department.
    pub department_id: Option<DepartmentId>,
    /// The Company.
    pub tenant_id: TenantId,
    /// The subject.
    pub subject: &'a str,
    /// The Requester's email address.
    pub requester_email: Option<&'a str>,
}

/// What an Administrator sets on a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTypeRuleInput {
    /// Name.
    pub name: String,
    /// The Ticket Type it sets.
    pub ticket_type_id: Uuid,
    /// Conditions.
    pub conditions: TicketTypeRuleConditions,
    /// Evaluation order (lowest first).
    pub sort_order: i32,
    /// Inactive rules are skipped.
    pub is_active: bool,
}

/// A Ticket Type Rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketTypeRule {
    id: Uuid,
    input: TicketTypeRuleInput,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl TicketTypeRuleInput {
    fn validate(mut self) -> Result<Self, TicketError> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 100 {
            return Err(TicketError::invalid("name", "must be 1 to 100 characters"));
        }
        self.conditions = self.conditions.validate()?;
        Ok(self)
    }
}

impl TicketTypeRule {
    /// Creates a rule.
    ///
    /// # Errors
    ///
    /// Invalid name or no conditions.
    pub fn create(input: TicketTypeRuleInput, now: DateTime<Utc>) -> Result<Self, TicketError> {
        Ok(Self {
            id: Uuid::now_v7(),
            input: input.validate()?,
            created_at: now,
            updated_at: now,
        })
    }

    /// Rebuilds a rule from storage.
    #[must_use]
    pub const fn restore(
        id: Uuid,
        input: TicketTypeRuleInput,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            input,
            created_at,
            updated_at,
        }
    }

    /// Changes the rule.
    ///
    /// # Errors
    ///
    /// Invalid input.
    pub fn update(
        &mut self,
        input: TicketTypeRuleInput,
        now: DateTime<Utc>,
    ) -> Result<(), TicketError> {
        self.input = input.validate()?;
        self.updated_at = now;
        Ok(())
    }

    /// ID.
    #[must_use]
    pub const fn id(&self) -> Uuid {
        self.id
    }
    /// Everything the Administrator set.
    #[must_use]
    pub const fn input(&self) -> &TicketTypeRuleInput {
        &self.input
    }
    /// Created.
    #[must_use]
    pub const fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
    /// Last changed.
    #[must_use]
    pub const fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }
}

/// The Ticket Type the first matching active rule sets, in `sort_order`; rules whose
/// type is not in `active_types` are skipped.
#[must_use]
pub fn choose_type(
    rules: &[TicketTypeRule],
    subject: &RuleSubject<'_>,
    active_types: &[Uuid],
) -> Option<Uuid> {
    let mut ordered: Vec<&TicketTypeRule> = rules.iter().filter(|r| r.input.is_active).collect();
    ordered.sort_by_key(|r| (r.input.sort_order, r.id));
    ordered
        .into_iter()
        .filter(|r| active_types.contains(&r.input.ticket_type_id))
        .find(|r| r.input.conditions.matches(subject))
        .map(|r| r.input.ticket_type_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(order: i32, ticket_type: Uuid, conditions: TicketTypeRuleConditions) -> TicketTypeRule {
        TicketTypeRule::create(
            TicketTypeRuleInput {
                name: format!("rule {order}"),
                ticket_type_id: ticket_type,
                conditions,
                sort_order: order,
                is_active: true,
            },
            Utc::now(),
        )
        .unwrap()
    }

    fn subject<'a>(tenant: TenantId, text: &'a str, email: &'a str) -> RuleSubject<'a> {
        RuleSubject {
            channel: Channel::Email,
            mailbox_id: None,
            origin_id: None,
            department_id: None,
            tenant_id: tenant,
            subject: text,
            requester_email: Some(email),
        }
    }

    #[test]
    fn first_matching_rule_in_order_wins_and_all_conditions_must_match() {
        let (incident, request) = (Uuid::now_v7(), Uuid::now_v7());
        let tenant = TenantId::new();
        let rules = vec![
            rule(
                20,
                request,
                TicketTypeRuleConditions {
                    channels: vec![Channel::Email],
                    ..Default::default()
                },
            ),
            rule(
                10,
                incident,
                TicketTypeRuleConditions {
                    channels: vec![Channel::Email],
                    subject_keywords: vec!["Outage".into(), "is down".into()],
                    ..Default::default()
                },
            ),
        ];
        let types = [incident, request];
        assert_eq!(
            choose_type(&rules, &subject(tenant, "VPN IS DOWN", "a@b.com"), &types),
            Some(incident)
        );
        assert_eq!(
            choose_type(&rules, &subject(tenant, "New laptop", "a@b.com"), &types),
            Some(request)
        );
        let mut portal = subject(tenant, "VPN is down", "a@b.com");
        portal.channel = Channel::Portal;
        assert_eq!(choose_type(&rules, &portal, &types), None);
        assert_eq!(
            choose_type(&rules, &subject(tenant, "outage", "a@b.com"), &[request]),
            Some(request),
            "a rule whose type is inactive is skipped"
        );
    }

    #[test]
    fn domains_match_subdomains_and_companies_scope_rules() {
        let t = Uuid::now_v7();
        let (acme, other) = (TenantId::new(), TenantId::new());
        let rules = vec![rule(
            1,
            t,
            TicketTypeRuleConditions {
                requester_domains: vec!["@Acme.example.".into()],
                company_ids: vec![acme],
                ..Default::default()
            },
        )];
        assert_eq!(
            rules[0].input().conditions.requester_domains,
            ["acme.example"]
        );
        assert_eq!(
            choose_type(&rules, &subject(acme, "x", "jo@eu.acme.example"), &[t]),
            Some(t)
        );
        assert_eq!(
            choose_type(&rules, &subject(acme, "x", "jo@notacme.example"), &[t]),
            None
        );
        assert_eq!(
            choose_type(&rules, &subject(other, "x", "jo@acme.example"), &[t]),
            None
        );
    }

    #[test]
    fn a_rule_needs_a_condition() {
        assert!(
            TicketTypeRule::create(
                TicketTypeRuleInput {
                    name: "all".into(),
                    ticket_type_id: Uuid::now_v7(),
                    conditions: TicketTypeRuleConditions {
                        subject_keywords: vec!["  ".into()],
                        ..Default::default()
                    },
                    sort_order: 0,
                    is_active: true,
                },
                Utc::now()
            )
            .is_err()
        );
    }
}
