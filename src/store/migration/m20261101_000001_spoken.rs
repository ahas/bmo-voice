//! The spoken words of each synthesis, kept beside its written words.

use sea_orm_migration::prelude::*;

pub const NAME: &str = "m20261101_000001_spoken";

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    NAME
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(include_str!("spoken.sql")).await?;

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration("the engine's tables are kept; its database is dropped instead".into()))
  }
}
