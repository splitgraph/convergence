//! Provides a DataFusion-powered implementation of the [Engine] trait.

use std::ops::Deref;
use crate::table::{record_batch_to_rows, schema_to_field_desc};
use async_trait::async_trait;
use convergence::engine::{Engine, Portal};
use convergence::protocol::{ErrorResponse, FieldDescription, SqlState};
use convergence::protocol_ext::DataRowBatch;
use datafusion::error::DataFusionError;
use datafusion::prelude::*;
use sqlparser::ast::Statement;
use std::sync::Arc;

fn df_err_to_sql(err: DataFusionError) -> ErrorResponse {
	ErrorResponse::error(SqlState::DATA_EXCEPTION, err.to_string())
}

/// A portal built using a logical DataFusion query plan.
pub struct DataFusionPortal {
	df: Arc<DataFrame>,
}

#[async_trait]
impl Portal for DataFusionPortal {
	async fn fetch(&mut self, batch: &mut DataRowBatch) -> Result<(), ErrorResponse> {
		for arrow_batch in self.df.deref().clone().collect().await.map_err(df_err_to_sql)? {
			record_batch_to_rows(&arrow_batch, batch)?;
		}
		Ok(())
	}
}

/// An engine instance using DataFusion for catalogue management and queries.
pub struct DataFusionEngine {
	ctx: SessionContext,
}

impl DataFusionEngine {
	/// Creates a new engine instance using the given DataFusion execution context.
	pub fn new(ctx: SessionContext) -> Self {
		Self { ctx }
	}
}

#[async_trait]
impl Engine for DataFusionEngine {
	type PortalType = DataFusionPortal;

	async fn prepare(&mut self, statement: &Statement) -> Result<Vec<FieldDescription>, ErrorResponse> {
		let plan = self.ctx.sql(&statement.to_string()).await.map_err(df_err_to_sql)?;
		schema_to_field_desc(&plan.schema().clone().into())
	}

	async fn create_portal(&mut self, statement: &Statement) -> Result<Self::PortalType, ErrorResponse> {
		let df = self.ctx.sql(&statement.to_string()).await.map_err(df_err_to_sql)?;
		Ok(DataFusionPortal { df: df.into() })
	}
}
