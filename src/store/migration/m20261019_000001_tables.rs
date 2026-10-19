//! Every table of the store with its indexes.

use sea_orm_migration::prelude::*;

pub const NAME: &str = "m20261019_000001_tables";

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    NAME
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(include_str!("tables.sql")).await?;

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration(
      "the store's first migration is not undone; its database is dropped instead".into(),
    ))
  }
}
