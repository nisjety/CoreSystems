"""
BigQuery Sync Manager for Contifico Service
Handles automatic table creation, schema detection, and historical data tracking.
"""

import logging
from datetime import datetime
from typing import Any, Dict, List, Optional
from google.cloud import bigquery
from google.cloud.exceptions import NotFound
import hashlib
import json

logger = logging.getLogger(__name__)


class BigQuerySyncManager:
    """Manages BigQuery table creation and data synchronization with historical tracking."""
    
    def __init__(self, project_id: str, dataset_id: str, location: str = "europe-west1"):
        """
        Initialize BigQuery Sync Manager.
        
        Args:
            project_id: GCP project ID
            dataset_id: BigQuery dataset ID
            location: BigQuery location/region
        """
        self.project_id = project_id
        self.dataset_id = dataset_id
        self.location = location
        self.client = bigquery.Client(project=project_id, location=location)
        
        # Ensure dataset exists
        self._ensure_dataset_exists()
    
    def _ensure_dataset_exists(self):
        """Create dataset if it doesn't exist."""
        try:
            dataset_ref = f"{self.project_id}.{self.dataset_id}"
            self.client.get_dataset(dataset_ref)
            logger.info(f"Dataset {dataset_ref} exists")
        except NotFound:
            dataset = bigquery.Dataset(dataset_ref)
            dataset.location = self.location
            dataset.description = "Contifico ERP data with historical tracking"
            dataset = self.client.create_dataset(dataset, timeout=30)
            logger.info(f"Created dataset {dataset_ref}")
    
    def _python_type_to_bq_type(self, value: Any) -> str:
        """Convert Python value to BigQuery field type."""
        if value is None:
            return "STRING"  # Default to STRING for null values
        
        if isinstance(value, bool):
            return "BOOLEAN"
        elif isinstance(value, int):
            return "INTEGER"
        elif isinstance(value, float):
            return "FLOAT"
        elif isinstance(value, dict):
            return "JSON"
        elif isinstance(value, list):
            return "JSON"  # Store arrays as JSON
        elif isinstance(value, str):
            # Try to detect if it's a timestamp
            if self._is_timestamp(value):
                return "TIMESTAMP"
            return "STRING"
        else:
            return "STRING"
    
    def _is_timestamp(self, value: str) -> bool:
        """Check if string looks like a timestamp."""
        timestamp_patterns = [
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%dT%H:%M:%SZ",
            "%Y-%m-%dT%H:%M:%S.%fZ",
        ]
        for pattern in timestamp_patterns:
            try:
                datetime.strptime(value[:19], pattern[:19])
                return True
            except (ValueError, AttributeError):
                continue
        return False
    
    def _infer_schema_from_data(self, data: List[Dict[str, Any]]) -> List[bigquery.SchemaField]:
        """
        Infer BigQuery schema from data samples.
        
        Args:
            data: List of dictionaries representing records
            
        Returns:
            List of BigQuery SchemaField objects
        """
        if not data:
            return []
        
        # Collect all fields from all records
        field_types = {}
        
        for record in data[:100]:  # Sample first 100 records
            for key, value in record.items():
                if key not in field_types:
                    field_types[key] = set()
                field_types[key].add(self._python_type_to_bq_type(value))
        
        # Create schema fields
        schema = []
        for field_name, types in field_types.items():
            # Use most specific type, prefer non-STRING
            bq_type = "STRING"
            if "JSON" in types:
                bq_type = "JSON"
            elif "FLOAT" in types:
                bq_type = "FLOAT"
            elif "INTEGER" in types:
                bq_type = "INTEGER"
            elif "BOOLEAN" in types:
                bq_type = "BOOLEAN"
            elif "TIMESTAMP" in types:
                bq_type = "TIMESTAMP"
            
            schema.append(
                bigquery.SchemaField(
                    field_name,
                    bq_type,
                    mode="NULLABLE"
                )
            )
        
        # Add metadata fields for historical tracking
        metadata_fields = [
            bigquery.SchemaField("_record_id", "STRING", mode="REQUIRED", description="Unique record identifier"),
            bigquery.SchemaField("_data_hash", "STRING", mode="REQUIRED", description="Hash of record data for change detection"),
            bigquery.SchemaField("_ingestion_timestamp", "TIMESTAMP", mode="REQUIRED", description="When record was ingested"),
            bigquery.SchemaField("_valid_from", "TIMESTAMP", mode="REQUIRED", description="When this version became valid"),
            bigquery.SchemaField("_valid_to", "TIMESTAMP", mode="NULLABLE", description="When this version stopped being valid"),
            bigquery.SchemaField("_is_current", "BOOLEAN", mode="REQUIRED", description="Whether this is the current version"),
            bigquery.SchemaField("_source_api", "STRING", mode="REQUIRED", description="Source API endpoint"),
        ]
        
        return schema + metadata_fields
    
    def _calculate_record_hash(self, record: Dict[str, Any]) -> str:
        """Calculate hash of record data for change detection."""
        # Create a stable JSON string (sorted keys)
        record_json = json.dumps(record, sort_keys=True)
        return hashlib.sha256(record_json.encode()).hexdigest()
    
    def create_or_update_table(
        self,
        table_name: str,
        data_sample: List[Dict[str, Any]],
        description: Optional[str] = None
    ) -> bool:
        """
        Create or update BigQuery table based on data sample.
        
        Args:
            table_name: Name of the table
            data_sample: Sample data to infer schema
            description: Table description
            
        Returns:
            True if table was created/updated successfully
        """
        try:
            table_ref = f"{self.project_id}.{self.dataset_id}.{table_name}"
            
            # Infer schema from data
            schema = self._infer_schema_from_data(data_sample)
            
            if not schema:
                logger.error(f"Could not infer schema from data for {table_name}")
                return False
            
            try:
                # Check if table exists
                existing_table = self.client.get_table(table_ref)
                logger.info(f"Table {table_ref} already exists")
                
                # TODO: Implement schema evolution (add new columns if needed)
                # For now, we'll use the existing table
                return True
                
            except NotFound:
                # Create new table
                table = bigquery.Table(table_ref, schema=schema)
                table.description = description or f"Contifico {table_name} with historical tracking"
                
                # Enable clustering on common query fields
                table.clustering_fields = ["_is_current", "_valid_from"]
                
                table = self.client.create_table(table)
                logger.info(f"Created table {table_ref} with {len(schema)} fields")
                return True
                
        except Exception as e:
            logger.error(f"Error creating/updating table {table_name}: {str(e)}")
            return False
    
    def sync_data(
        self,
        table_name: str,
        data: List[Dict[str, Any]],
        id_field: str,
        source_api: str
    ) -> Dict[str, Any]:
        """
        Sync data to BigQuery with historical tracking.
        
        Args:
            table_name: Target table name
            data: List of records to sync
            id_field: Field name used as primary identifier
            source_api: Source API endpoint
            
        Returns:
            Dictionary with sync statistics
        """
        if not data:
            return {"status": "skipped", "reason": "no data", "records": 0}
        
        try:
            # Ensure table exists
            if not self.create_or_update_table(table_name, data):
                return {"status": "error", "reason": "table creation failed"}
            
            table_ref = f"{self.project_id}.{self.dataset_id}.{table_name}"
            current_timestamp = datetime.utcnow().isoformat()
            
            # Prepare records with metadata
            records_to_insert = []
            records_to_update = []
            
            for record in data:
                if id_field not in record:
                    logger.warning(f"Record missing ID field '{id_field}', skipping")
                    continue
                
                record_id = str(record[id_field])
                data_hash = self._calculate_record_hash(record)
                
                # Check if record already exists and is different
                query = f"""
                SELECT _data_hash, _record_id
                FROM `{table_ref}`
                WHERE _record_id = @record_id
                AND _is_current = TRUE
                LIMIT 1
                """
                
                job_config = bigquery.QueryJobConfig(
                    query_parameters=[
                        bigquery.ScalarQueryParameter("record_id", "STRING", record_id)
                    ]
                )
                
                try:
                    query_job = self.client.query(query, job_config=job_config)
                    existing_records = list(query_job.result())
                    
                    if existing_records:
                        existing_hash = existing_records[0]["_data_hash"]
                        
                        if existing_hash != data_hash:
                            # Record has changed - mark old version as invalid
                            records_to_update.append(record_id)
                            
                            # Add new version
                            enriched_record = {
                                **record,
                                "_record_id": record_id,
                                "_data_hash": data_hash,
                                "_ingestion_timestamp": current_timestamp,
                                "_valid_from": current_timestamp,
                                "_valid_to": None,
                                "_is_current": True,
                                "_source_api": source_api,
                            }
                            records_to_insert.append(enriched_record)
                        # else: Record unchanged, skip
                    else:
                        # New record
                        enriched_record = {
                            **record,
                            "_record_id": record_id,
                            "_data_hash": data_hash,
                            "_ingestion_timestamp": current_timestamp,
                            "_valid_from": current_timestamp,
                            "_valid_to": None,
                            "_is_current": True,
                            "_source_api": source_api,
                        }
                        records_to_insert.append(enriched_record)
                        
                except Exception as e:
                    logger.error(f"Error checking existing record {record_id}: {str(e)}")
                    continue
            
            # Update old versions (mark as no longer current)
            if records_to_update:
                update_query = f"""
                UPDATE `{table_ref}`
                SET _is_current = FALSE,
                    _valid_to = @valid_to
                WHERE _record_id IN UNNEST(@record_ids)
                AND _is_current = TRUE
                """
                
                job_config = bigquery.QueryJobConfig(
                    query_parameters=[
                        bigquery.ScalarQueryParameter("valid_to", "TIMESTAMP", current_timestamp),
                        bigquery.ArrayQueryParameter("record_ids", "STRING", records_to_update),
                    ]
                )
                
                update_job = self.client.query(update_query, job_config=job_config)
                update_job.result()
                logger.info(f"Marked {len(records_to_update)} old versions as invalid")
            
            # Insert new/updated records
            if records_to_insert:
                errors = self.client.insert_rows_json(table_ref, records_to_insert)
                
                if errors:
                    logger.error(f"Errors inserting rows: {errors}")
                    return {
                        "status": "partial",
                        "records_inserted": len(records_to_insert) - len(errors),
                        "records_updated": len(records_to_update),
                        "errors": errors[:10]  # First 10 errors
                    }
                
                logger.info(f"Inserted {len(records_to_insert)} records to {table_name}")
            
            return {
                "status": "success",
                "records_inserted": len(records_to_insert),
                "records_updated": len(records_to_update),
                "total_processed": len(data),
                "table": table_ref,
                "timestamp": current_timestamp
            }
            
        except Exception as e:
            logger.error(f"Error syncing data to {table_name}: {str(e)}")
            return {"status": "error", "reason": str(e)}
    
    def get_table_info(self, table_name: str) -> Dict[str, Any]:
        """Get information about a specific table."""
        try:
            table_ref = f"{self.project_id}.{self.dataset_id}.{table_name}"
            table = self.client.get_table(table_ref)
            
            # Get row count with backticks around _is_current and alias as current_count
            query = f"SELECT COUNT(*) as total, COUNTIF(`_is_current`) as current_count FROM `{table_ref}`"
            result = list(self.client.query(query).result())[0]
            
            return {
                "exists": True,
                "table_id": table.table_id,
                "created": table.created.isoformat() if table.created else None,
                "modified": table.modified.isoformat() if table.modified else None,
                "num_rows": table.num_rows,
                "total_records": result["total"],
                "current_records": result["current_count"],
                "size_bytes": table.num_bytes,
                "schema_fields": len(table.schema),
            }
        except NotFound:
            return {"exists": False}
        except Exception as e:
            logger.error(f"Error getting table info for {table_name}: {str(e)}")
            return {"exists": False, "error": str(e)}
    
    def query_historical_data(
        self,
        table_name: str,
        record_id: str,
        order_by: str = "_valid_from DESC"
    ) -> List[Dict[str, Any]]:
        """
        Query all historical versions of a specific record.
        
        Args:
            table_name: Table name
            record_id: Record identifier
            order_by: Sort order (default: newest first)
            
        Returns:
            List of all versions of the record
        """
        try:
            table_ref = f"{self.project_id}.{self.dataset_id}.{table_name}"
            query = f"""
            SELECT *
            FROM `{table_ref}`
            WHERE _record_id = @record_id
            ORDER BY {order_by}
            """
            
            job_config = bigquery.QueryJobConfig(
                query_parameters=[
                    bigquery.ScalarQueryParameter("record_id", "STRING", record_id)
                ]
            )
            
            query_job = self.client.query(query, job_config=job_config)
            results = query_job.result()
            
            return [dict(row) for row in results]
            
        except Exception as e:
            logger.error(f"Error querying historical data: {str(e)}")
            return []
