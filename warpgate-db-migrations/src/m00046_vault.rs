use sea_orm::Schema;
use sea_orm_migration::prelude::*;

pub mod vault_item {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::ForeignKeyAction;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
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
                Self::CreatedByUser => Entity::belongs_to(crate::m00008_users::user::Entity)
                    .from(Column::CreatedByUserId)
                    .to(crate::m00008_users::user::Column::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .into(),
                Self::AccessEvents => Entity::has_many(super::vault_access_event::Entity)
                    .from(Column::Id)
                    .to(super::vault_access_event::Column::VaultItemId)
                    .into(),
            }
        }
    }

    impl Related<crate::m00008_users::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::CreatedByUser.def()
        }
    }

    impl Related<super::vault_access_event::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::AccessEvents.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod vault_access_event {
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::ForeignKeyAction;
    use time::OffsetDateTime;
    use uuid::Uuid;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
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
                Self::VaultItem => Entity::belongs_to(super::vault_item::Entity)
                    .from(Column::VaultItemId)
                    .to(super::vault_item::Column::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .into(),
                Self::ActorUser => Entity::belongs_to(crate::m00008_users::user::Entity)
                    .from(Column::ActorUserId)
                    .to(crate::m00008_users::user::Column::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .into(),
                Self::Session => Entity::belongs_to(crate::m00002_create_session::session::Entity)
                    .from(Column::SessionId)
                    .to(crate::m00002_create_session::session::Column::Id)
                    .on_delete(ForeignKeyAction::SetNull)
                    .into(),
            }
        }
    }

    impl Related<super::vault_item::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::VaultItem.def()
        }
    }

    impl Related<crate::m00008_users::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::ActorUser.def()
        }
    }

    impl Related<crate::m00002_create_session::session::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::Session.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00046_vault"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let builder = manager.get_database_backend();
        let schema = Schema::new(builder);

        manager
            .create_table(schema.create_table_from_entity(vault_item::Entity))
            .await?;

        manager
            .create_table(schema.create_table_from_entity(vault_access_event::Entity))
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_vault_items_deleted_at")
                    .table(vault_item::Entity)
                    .col(vault_item::Column::DeletedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_vault_access_events_item_timestamp")
                    .table(vault_access_event::Entity)
                    .col(vault_access_event::Column::VaultItemId)
                    .col(vault_access_event::Column::Timestamp)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(vault_access_event::Entity).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(vault_item::Entity).to_owned())
            .await?;

        Ok(())
    }
}
