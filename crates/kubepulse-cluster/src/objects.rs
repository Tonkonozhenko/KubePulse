//! Single-object requests: opening, listing, changing, deleting, and related
//! events.

use k8s_openapi::api::{
    authorization::v1::{ResourceAttributes, SelfSubjectAccessReview, SelfSubjectAccessReviewSpec},
    core::v1::Event,
};
use kube::{
    Api, Client,
    api::{DeleteParams, DynamicObject, ListParams, PostParams},
};
use serde_json::Value;

use crate::{
    resource_watch::{SessionHandle, api_resource},
    session::ResourceKind,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectTarget {
    pub kind: ResourceKind,
    pub namespace: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectError {
    Denied(String),
    /// The object changed after it was read, so the server refused the write.
    Conflict(String),
    NotFound(String),
    Failed(String),
    NotConnected,
}

impl std::fmt::Display for ObjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(message)
            | Self::Conflict(message)
            | Self::NotFound(message)
            | Self::Failed(message) => f.write_str(message),
            Self::NotConnected => f.write_str("the cluster is not connected"),
        }
    }
}

impl std::error::Error for ObjectError {}

pub(crate) fn object_error(error: kube::Error) -> ObjectError {
    match error {
        kube::Error::Api(status) if status.code == 403 => ObjectError::Denied(status.message),
        kube::Error::Api(status) if status.code == 404 => ObjectError::NotFound(status.message),
        kube::Error::Api(status) if status.code == 409 => ObjectError::Conflict(status.message),
        kube::Error::Api(status) => ObjectError::Failed(status.message),
        _ => ObjectError::Failed("the request failed".to_owned()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    pub kind: String,
    pub reason: String,
    pub message: String,
    pub count: i32,
    pub last_seen: Option<String>,
}

/// Whether a write takes effect or only runs the server's validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    DryRun,
    Apply,
}

impl WriteMode {
    fn params(self) -> PostParams {
        PostParams {
            dry_run: self == Self::DryRun,
            field_manager: Some("kubepulse".to_owned()),
        }
    }
}

/// Whether the user's account may change an object, and if not, why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateAccess {
    pub allowed: bool,
    pub reason: Option<String>,
}

fn write_body(object: Value) -> Result<DynamicObject, ObjectError> {
    serde_json::from_value(object).map_err(|error| ObjectError::Failed(error.to_string()))
}

fn api(client: Client, kind: &ResourceKind, namespace: Option<&str>) -> Api<DynamicObject> {
    let resource = api_resource(kind);
    match namespace {
        Some(namespace) if kind.namespaced => Api::namespaced_with(client, namespace, &resource),
        _ => Api::all_with(client, &resource),
    }
}

fn to_value(object: DynamicObject) -> Value {
    let metadata = serde_json::to_value(&object.metadata).expect("object metadata serializes");
    let mut data = object.data;
    if let Some(fields) = data.as_object_mut() {
        fields.insert("metadata".to_owned(), metadata);
        if let Some(types) = object.types {
            fields.insert("apiVersion".to_owned(), types.api_version.into());
            fields.insert("kind".to_owned(), types.kind.into());
        }
    }
    data
}

impl SessionHandle {
    async fn connected(&self) -> Result<Client, ObjectError> {
        self.client().await.ok_or(ObjectError::NotConnected)
    }

    /// The whole object, with `managedFields` included.
    pub async fn get_object(&self, target: &ObjectTarget) -> Result<Value, ObjectError> {
        let client = self.connected().await?;
        let api = api(client, &target.kind, target.namespace.as_deref());
        api.get(&target.name)
            .await
            .map(to_value)
            .map_err(object_error)
    }

    pub async fn list_objects(
        &self,
        kind: &ResourceKind,
        namespace: Option<&str>,
        labels: Option<&str>,
    ) -> Result<Vec<Value>, ObjectError> {
        let client = self.connected().await?;
        let mut params = ListParams::default();
        if let Some(labels) = labels {
            params = params.labels(labels);
        }
        let list = api(client, kind, namespace)
            .list(&params)
            .await
            .map_err(object_error)?;
        Ok(list.items.into_iter().map(to_value).collect())
    }

    /// Writes the whole object. The `resourceVersion` it carries makes the
    /// server refuse the write if the object has changed since.
    pub async fn replace_object(
        &self,
        target: &ObjectTarget,
        object: Value,
        mode: WriteMode,
    ) -> Result<Value, ObjectError> {
        let client = self.connected().await?;
        let api = api(client, &target.kind, target.namespace.as_deref());
        api.replace(&target.name, &mode.params(), &write_body(object)?)
            .await
            .map(to_value)
            .map_err(object_error)
    }

    pub async fn create_object(
        &self,
        target: &ObjectTarget,
        object: Value,
        mode: WriteMode,
    ) -> Result<Value, ObjectError> {
        let client = self.connected().await?;
        let api = api(client, &target.kind, target.namespace.as_deref());
        api.create(&mode.params(), &write_body(object)?)
            .await
            .map(to_value)
            .map_err(object_error)
    }

    pub async fn update_access(&self, target: &ObjectTarget) -> Result<UpdateAccess, ObjectError> {
        let client = self.connected().await?;
        let review = SelfSubjectAccessReview {
            spec: SelfSubjectAccessReviewSpec {
                resource_attributes: Some(ResourceAttributes {
                    group: Some(target.kind.group.clone()),
                    name: Some(target.name.clone()),
                    namespace: target.namespace.clone(),
                    resource: Some(target.kind.plural.clone()),
                    verb: Some("update".to_owned()),
                    ..ResourceAttributes::default()
                }),
                ..SelfSubjectAccessReviewSpec::default()
            },
            ..SelfSubjectAccessReview::default()
        };
        let reviewed = Api::<SelfSubjectAccessReview>::all(client)
            .create(&PostParams::default(), &review)
            .await
            .map_err(object_error)?;
        let status = reviewed.status.unwrap_or_default();
        Ok(UpdateAccess {
            allowed: status.allowed,
            reason: status.reason.filter(|reason| !reason.is_empty()),
        })
    }

    pub async fn delete_object(&self, target: &ObjectTarget) -> Result<(), ObjectError> {
        let client = self.connected().await?;
        let api = api(client, &target.kind, target.namespace.as_deref());
        api.delete(&target.name, &DeleteParams::default())
            .await
            .map_err(object_error)?;
        Ok(())
    }

    /// Events about the object with this UID, newest first.
    pub async fn events_for(
        &self,
        namespace: Option<&str>,
        uid: &str,
    ) -> Result<Vec<EventRecord>, ObjectError> {
        let client = self.connected().await?;
        let api: Api<Event> = match namespace {
            Some(namespace) => Api::namespaced(client, namespace),
            None => Api::all(client),
        };
        let params = ListParams::default().fields(&format!("involvedObject.uid={uid}"));
        let list = api.list(&params).await.map_err(object_error)?;
        let mut records: Vec<EventRecord> = list
            .items
            .into_iter()
            .map(|event| EventRecord {
                kind: event.type_.unwrap_or_default(),
                reason: event.reason.unwrap_or_default(),
                message: event.message.unwrap_or_default(),
                count: event.count.unwrap_or(1),
                last_seen: event.last_timestamp.map(|time| time.0.to_string()),
            })
            .collect();
        records.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        Ok(records)
    }
}
