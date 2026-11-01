//! The language each synthesis is said in, kept beside its words.

use sea_orm_migration::prelude::*;

pub const NAME: &str = "m20261101_000002_language";

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    NAME
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(include_str!("language.sql")).await?;

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration("the engine's tables are kept; its database is dropped instead".into()))
  }
}
