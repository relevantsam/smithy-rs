/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::client::auth::AuthSchemeId;
use crate::client::orchestrator::Future;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreAppend, StoreReplace};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(feature = "http-auth")]
pub mod http;

/// Resolves an identity for a request.
pub trait IdentityResolver: Send + Sync + Debug {
    fn resolve_identity(&self, config_bag: &ConfigBag) -> Future<Identity>;
}

/// Container for a shared identity resolver.
#[derive(Clone, Debug)]
pub struct SharedIdentityResolver(Arc<dyn IdentityResolver>);

impl SharedIdentityResolver {
    /// Creates a new [`SharedIdentityResolver`] from the given resolver.
    pub fn new(resolver: impl IdentityResolver + 'static) -> Self {
        Self(Arc::new(resolver))
    }
}

impl IdentityResolver for SharedIdentityResolver {
    fn resolve_identity(&self, config_bag: &ConfigBag) -> Future<Identity> {
        self.0.resolve_identity(config_bag)
    }
}

/// An identity resolver paired with an auth scheme ID that it resolves for.
#[derive(Clone, Debug)]
pub(crate) struct ConfiguredIdentityResolver {
    auth_scheme: AuthSchemeId,
    identity_resolver: SharedIdentityResolver,
}

impl ConfiguredIdentityResolver {
    /// Creates a new [`ConfiguredIdentityResolver`] from the given auth scheme and identity resolver.
    pub(crate) fn new(
        auth_scheme: AuthSchemeId,
        identity_resolver: SharedIdentityResolver,
    ) -> Self {
        Self {
            auth_scheme,
            identity_resolver,
        }
    }

    /// Returns the auth scheme ID.
    pub(crate) fn scheme_id(&self) -> AuthSchemeId {
        self.auth_scheme
    }

    /// Returns the identity resolver.
    pub(crate) fn identity_resolver(&self) -> SharedIdentityResolver {
        self.identity_resolver.clone()
    }
}

impl Storable for ConfiguredIdentityResolver {
    type Storer = StoreAppend<Self>;
}

#[derive(Clone, Debug, Default)]
pub struct IdentityResolvers {
    identity_resolvers: Vec<ConfiguredIdentityResolver>,
}

impl Storable for IdentityResolvers {
    type Storer = StoreReplace<IdentityResolvers>;
}

impl IdentityResolvers {
    pub(crate) fn new<'a>(resolvers: impl Iterator<Item = &'a ConfiguredIdentityResolver>) -> Self {
        let identity_resolvers: Vec<_> = resolvers.cloned().collect();
        if identity_resolvers.is_empty() {
            tracing::warn!("no identity resolvers available for this request");
        }
        Self { identity_resolvers }
    }

    pub fn identity_resolver(&self, scheme_id: AuthSchemeId) -> Option<SharedIdentityResolver> {
        self.identity_resolvers
            .iter()
            .find(|pair| pair.scheme_id() == scheme_id)
            .map(|pair| pair.identity_resolver())
    }
}

#[derive(Clone, Debug)]
pub struct Identity {
    data: Arc<dyn Any + Send + Sync>,
    expiration: Option<SystemTime>,
}

impl Identity {
    pub fn new(data: impl Any + Send + Sync, expiration: Option<SystemTime>) -> Self {
        Self {
            data: Arc::new(data),
            expiration,
        }
    }

    pub fn data<T: 'static>(&self) -> Option<&T> {
        self.data.downcast_ref()
    }

    pub fn expiration(&self) -> Option<&SystemTime> {
        self.expiration.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_send_sync() {
        fn is_send_sync<T: Send + Sync>(_: T) {}
        is_send_sync(Identity::new("foo", None));
    }

    #[test]
    fn create_retrieve_identity() {
        #[derive(Debug)]
        struct MyIdentityData {
            first: String,
            last: String,
        }

        let expiration = SystemTime::now();
        let identity = Identity::new(
            MyIdentityData {
                first: "foo".into(),
                last: "bar".into(),
            },
            Some(expiration),
        );

        assert_eq!("foo", identity.data::<MyIdentityData>().unwrap().first);
        assert_eq!("bar", identity.data::<MyIdentityData>().unwrap().last);
        assert_eq!(Some(&expiration), identity.expiration());
    }
}
