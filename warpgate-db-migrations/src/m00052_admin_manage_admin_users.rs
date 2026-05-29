use sea_orm_migration::prelude::*;

use crate::m00032_admin_roles::admin_role;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m00052_admin_manage_admin_users"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let alter_result = manager
            .alter_table(
                Table::alter()
                    .table(admin_role::Entity)
                    .add_column_if_not_exists(
                        ColumnDef::new(Alias::new("users_manage_admins"))
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await;

        if let Err(error) = alter_result
            && !is_duplicate_column_error(&error)
        {
            return Err(error);
        }

        let conn = manager.get_connection();
        let bool_true = match manager.get_database_backend() {
            sea_orm::DatabaseBackend::Postgres => "TRUE",
            _ => "1",
        };
        conn.execute_unprepared(&format!(
            "UPDATE admin_roles SET users_manage_admins = {bool_true} WHERE name = 'warpgate:admin'",
        ))
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(admin_role::Entity)
                    .drop_column(Alias::new("users_manage_admins"))
                    .to_owned(),
            )
            .await
    }
}

fn is_duplicate_column_error(error: &DbErr) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("duplicate column name")
        || (message.contains("already exists") && message.contains("users_manage_admins"))
}
