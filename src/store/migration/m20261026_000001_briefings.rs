//! The briefings the engine speaks: the synthesis each one is spoken by, and whether it plays by itself.

use sea_orm_migration::prelude::*;

pub const NAME: &str = "m20261026_000001_briefings";

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    NAME
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(include_str!("briefings.sql")).await?;

    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration("the engine's tables are kept; its database is dropped instead".into()))
  }
}
