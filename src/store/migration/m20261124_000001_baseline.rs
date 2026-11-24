//! The store's schema at 1.0.0, whole: the citext extension and every table with its indexes.

use sea_orm_migration::prelude::*;

pub const NAME: &str = "m20261124_000001_baseline";

pub struct Migration;

impl MigrationName for Migration {
  fn name(&self) -> &str {
    NAME
  }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
  async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.get_connection().execute_unprepared(include_str!("baseline.sql")).await?;
    Ok(())
  }

  async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
    Err(DbErr::Migration("the baseline is not undone; its database is dropped instead".into()))
  }
}
