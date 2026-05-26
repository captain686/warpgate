use sea_orm::entity::prelude::*;
use sea_orm::sea_query::ForeignKeyAction;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "vault_items")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub name: String,
    pub kind: String,
    #[sea_orm(column_type = "Text")]
    pub ciphertext: String,
    #[sea_orm(column_type = "Text")]
    pub nonce: String,
    #[sea_orm(column_type = "Text")]
    pub aad: String,
    pub key_fingerprint: String,
    pub version: i32,
    pub metadata: serde_json::Value,
    pub created: OffsetDateTime,
    pub updated: OffsetDateTime,
    pub created_by_user_id: Option<Uuid>,
    pub deleted_at: Option<OffsetDateTime>,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    CreatedByUser,
    AccessEvents,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::CreatedByUser => Entity::belongs_to(super::User::Entity)
                .from(Column::CreatedByUserId)
                .to(super::User::Column::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .into(),
            Self::AccessEvents => Entity::has_many(super::VaultAccessEvent::Entity)
                .from(Column::Id)
                .to(super::VaultAccessEvent::Column::VaultItemId)
                .into(),
        }
    }
}

impl Related<super::User::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::CreatedByUser.def()
    }
}

impl Related<super::VaultAccessEvent::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::AccessEvents.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
