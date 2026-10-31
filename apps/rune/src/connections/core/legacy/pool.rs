use super::*;
use crate::{
    connections::{
        leases::Entry,
        legacy::{ResolutionVersion, SelectionContext},
    },
    source::RouteOwner,
};
use std::sync::Arc;
pub(in crate::connections) struct RouteEntry {
    pub entry: Entry,
    version: ResolutionVersion,
    context: SelectionContext,
    owner: Arc<RouteOwner>,
}
impl Core {
    pub(in crate::connections) fn retain_route(
        &mut self,
        connection: Connection,
        profiles: Vec<crate::gateway::Profile>,
        version: ResolutionVersion,
        context: SelectionContext,
        actual_parent: Option<Lease>,
        tunnel: Option<Box<dyn Tunnel>>,
    ) -> Result<Lease, String> {
        let owner = Arc::new(
            connection
                .route_owner()
                .ok_or("The resolved route has no requested authority")?
                .clone(),
        );
        let key = (
            owner.fingerprint(),
            crate::client::connection_key(&connection)?,
        );
        if let Some(lease) = self
            .route_leases
            .get(&key)
            .filter(|entry| entry.version == version)
            .and_then(|entry| entry.entry.acquire())
        {
            return Ok(lease);
        }
        let (entry, lease) = Entry::routed(connection, profiles, actual_parent, tunnel)?;
        self.route_leases.insert(
            key,
            RouteEntry {
                entry,
                version,
                context,
                owner,
            },
        );
        Ok(lease)
    }
    pub(in crate::connections) fn retire_route_sources(&mut self, error: &str) {
        for route in self.route_leases.values_mut() {
            route.entry.fail(error);
        }
        self.route_leases.clear();
    }
    pub(in crate::connections) fn poll_route_leases(&mut self) {
        let store = self.registry.legacy.clone().unwrap_or_default();
        let registry = &self.registry;
        self.route_leases.retain(|_, route| {
            if !route.entry.referenced() {
                return false;
            }
            if let Some(error) = route.entry.exited() {
                route.entry.fail(&error);
                return false;
            }
            let mut context = route.context.clone();
            context.primary = registry
                .primary
                .as_ref()
                .and_then(|id| registry.rows.iter().find(|row| &row.id == id))
                .cloned();
            let requested_saved_valid =
                route
                    .owner
                    .registry_identity()
                    .is_none_or(|(id, revision)| {
                        registry
                            .rows
                            .iter()
                            .any(|row| row.id == id && row.revision == revision)
                    });
            let dependencies = match route.version.native_identity() {
                Some((id, revision)) => registry
                    .rows
                    .iter()
                    .any(|row| row.id == id && row.revision == revision),
                None => store.validates_resolution(&route.version, &context),
            };
            if !requested_saved_valid || !dependencies || route.entry.acquire().is_none() {
                route.entry.fail(
                    "The resolved route or its captured source changed; reconnect this scope",
                );
                return false;
            }
            true
        });
    }
}
