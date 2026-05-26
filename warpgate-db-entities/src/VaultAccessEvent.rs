use sea_orm::entity::prelude::*;
use sea_orm::sea_query::ForeignKeyAction;
use serde::Serialize;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "vault_access_events")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub vault_item_id: Uuid,
    pub action: String,
    pub actor_user_id: Option<Uuid>,
    pub session_id: Option<Uuid>,
    pub timestamp: OffsetDateTime,
    pub success: bool,
    #[sea_orm(column_type = "Text", nullable)]
    pub reason: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {
    VaultItem,
    ActorUser,
    Session,
}

impl RelationTrait for Relation {
    fn def(&self) -> RelationDef {
        match self {
            Self::VaultItem => Entity::belongs_to(super::VaultItem::Entity)
                .from(Column::VaultItemId)
                .to(super::VaultItem::Column::Id)
                .on_delete(ForeignKeyAction::Cascade)
                .into(),
            Self::ActorUser => Entity::belongs_to(super::User::Entity)
                .from(Column::ActorUserId)
                .to(super::User::Column::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .into(),
            Self::Session => Entity::belongs_to(super::Session::Entity)
                .from(Column::SessionId)
                .to(super::Session::Column::Id)
                .on_delete(ForeignKeyAction::SetNull)
                .into(),
        }
    }
}

impl Related<super::VaultItem::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::VaultItem.def()
    }
}

impl Related<super::User::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ActorUser.def()
    }
}

impl Related<super::Session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Session.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
