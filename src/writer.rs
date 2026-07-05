use crate::analyze::CrateData;

use parquet::arrow::AsyncArrowWriter;
use parquet::errors::ParquetError;
use parquet::file::metadata::ParquetMetaData;

use arrow::datatypes::{DataType, Schema, Field, FieldRef};
use serde_arrow::schema::{SchemaLike, TracingOptions};
use tokio::fs::File;
use std::sync::Arc;

pub struct Writer {
    writer: AsyncArrowWriter<File>
}

impl Writer {
    pub async fn new(file_path: &str) -> Writer {
        let file = File::create(file_path).await.expect("Could not create file.");

        let schema = Arc::new(Schema::new(vec![
            Field::new("Crate", DataType::Utf8, false),
            Field::new("Unsafe_Traits", DataType::UInt32, false),
            Field::new("Unsafe_Exprs", DataType::UInt32, false),
            Field::new("Unsafe_Impls", DataType::UInt32, false),
            Field::new("Unsafe_Funcs", DataType::UInt32, false),
            Field::new("Unsafe_Mods", DataType::UInt32, false),
            Field::new("FFI_Export_Funcs", DataType::UInt32, false),
            Field::new("FFI_Import_Funcs", DataType::UInt32, false),
        ]));

        let writer = AsyncArrowWriter::try_new(file, schema, None)
            .expect("Failed to instantiate AsyncArrowWriter.");

        return Writer { writer: writer }
    }

    pub async fn write(&mut self, data: &Vec<CrateData>) -> Result<(), ParquetError> {
        let fields = Vec::<FieldRef>::from_type::<CrateData>(TracingOptions::default())
            .expect("Parsing error from CrataData -> FieldRef");
        let batch = serde_arrow::to_record_batch(&fields, &data).unwrap();
        self.writer.write(&batch).await
    }

    pub async fn flush(&mut self) -> Result<(), ParquetError> {
        self.writer.flush().await
    }

    pub async fn close(self) -> Result<ParquetMetaData, ParquetError> {
        self.writer.close().await
    }
}
