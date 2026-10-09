//! Explicit domain populations and joins, independent of names and timestamp proximity.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvestigationPolicy {
    pub version: u32,
    pub roles: Vec<Role>,
    #[serde(default)]
    pub populations: Vec<Population>,
    #[serde(default)]
    pub relationships: Vec<Relationship>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub id: String,
    pub rule_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Population {
    pub id: String,
    pub entity: Entity,
    pub roles: Vec<String>,
    pub identity_fields: Vec<String>,
    pub grouping: Grouping,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Entity {
    Events,
    Attempts,
    Resources,
}
impl Entity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Events => "events",
            Self::Attempts => "attempts",
            Self::Resources => "resources",
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    Occurrence,
    Identity,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Relationship {
    pub id: String,
    pub source_role: String,
    pub target_role: String,
    pub join_fields: Vec<String>,
    pub required_scope_fields: Vec<String>,
    pub cardinality: Cardinality,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    OneToOne,
    ManyToOne,
    OneToMany,
}
impl InvestigationPolicy {
    pub(crate) fn validate(
        &self,
        config: &crate::config::AnalyzerConfig,
    ) -> std::result::Result<(), String> {
        let fail = || {
            Err("investigation policy requires version 1, bounded unique declarations, existing explicit rule/role IDs, and nonempty bounded identity/join/scope selectors".into())
        };
        if self.version != 1
            || self.roles.is_empty()
            || self.roles.len() > 32
            || self.populations.len() > 32
            || self.relationships.len() > 32
        {
            return fail();
        }
        let Some(classifier) = config.event_classifier() else {
            return fail();
        };
        let rules: BTreeSet<_> = classifier
            .schema()
            .rules
            .iter()
            .map(|rule| rule.id.as_str())
            .collect();
        let mut roles = BTreeSet::new();
        let names = |fields: &[String]| {
            !fields.is_empty()
                && fields.len() <= 16
                && fields
                    .iter()
                    .all(|field| !field.is_empty() && field.len() <= 4096)
                && fields.iter().collect::<BTreeSet<_>>().len() == fields.len()
        };
        for role in &self.roles {
            if !names(std::slice::from_ref(&role.id))
                || !roles.insert(role.id.as_str())
                || !names(&role.rule_ids)
                || role.rule_ids.iter().any(|id| !rules.contains(id.as_str()))
            {
                return fail();
            }
        }
        let mut ids = BTreeSet::new();
        for population in &self.populations {
            if !names(std::slice::from_ref(&population.id))
                || !ids.insert(population.id.as_str())
                || !names(&population.roles)
                || population
                    .roles
                    .iter()
                    .any(|id| !roles.contains(id.as_str()))
                || !names(&population.identity_fields)
            {
                return fail();
            }
            if matches!(population.entity, Entity::Resources)
                && !matches!(population.grouping, Grouping::Identity)
            {
                return Err("Distinct resources require identity grouping".into());
            }
        }
        ids.clear();
        for relationship in &self.relationships {
            if !names(std::slice::from_ref(&relationship.id))
                || !ids.insert(relationship.id.as_str())
                || !roles.contains(relationship.source_role.as_str())
                || !roles.contains(relationship.target_role.as_str())
                || !names(&relationship.join_fields)
                || !names(&relationship.required_scope_fields)
            {
                return fail();
            }
        }
        Ok(())
    }
}
