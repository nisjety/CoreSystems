import logging
from collections.abc import AsyncGenerator
from pathlib import Path

from sqlalchemy import inspect, text
from sqlalchemy.ext.asyncio import AsyncSession, async_sessionmaker, create_async_engine

from app.config import get_settings


settings = get_settings()
logger = logging.getLogger(__name__)
MIGRATIONS_DIR = Path(__file__).resolve().parent.parent / "migrations"
MIGRATION_LOCK_KEY = 661944154
CURRENT_IMPORT_JOBS_COLUMNS = {
    "id",
    "org_id",
    "user_id",
    "source_type",
    "status",
    "total_items",
    "processed_items",
    "failed_items",
    "error_message",
    "metadata",
    "created_at",
    "started_at",
    "completed_at",
}
CURRENT_IMPORT_JOB_ITEMS_COLUMNS = {
    "id",
    "job_id",
    "source_id",
    "source_name",
    "status",
    "error_message",
    "metadata",
    "created_at",
    "completed_at",
}

# Pool tuned for 0.25 CPU / 256MB resource limit:
# - pool_size=3  : base connections (was: default 5)
# - max_overflow=2: allow up to 5 total under burst (was: default 10 → 15 total)
# - pool_recycle : recycle connections every 30 min to avoid stale TCP state
# - pool_timeout : fail fast rather than queue requests indefinitely
engine = create_async_engine(
    settings.database_url,
    pool_pre_ping=True,
    pool_size=3,
    max_overflow=2,
    pool_recycle=1800,  # 30 minutes
    pool_timeout=30,
)
SessionLocal = async_sessionmaker(bind=engine, class_=AsyncSession, expire_on_commit=False)


def _quote_identifier(identifier: str) -> str:
    return '"' + identifier.replace('"', '""') + '"'


def _load_columns(sync_conn, table_name: str) -> dict[str, dict]:
    return {column["name"]: column for column in inspect(sync_conn).get_columns(table_name)}


def _next_legacy_table_name(sync_conn, table_name: str) -> str:
    existing_tables = set(inspect(sync_conn).get_table_names())
    candidate = f"{table_name}_legacy"
    suffix = 1
    while candidate in existing_tables:
        candidate = f"{table_name}_legacy_{suffix}"
        suffix += 1
    return candidate


def _archive_table(sync_conn, table_name: str) -> None:
    archived_name = _next_legacy_table_name(sync_conn, table_name)
    row_count = sync_conn.execute(
        text(f"SELECT COUNT(*) FROM {_quote_identifier(table_name)}")
    ).scalar_one()
    sync_conn.execute(
        text(
            f"ALTER TABLE {_quote_identifier(table_name)} "
            f"RENAME TO {_quote_identifier(archived_name)}"
        )
    )
    logger.warning(
        "Archived legacy imports table %s -> %s (%s rows)",
        table_name,
        archived_name,
        row_count,
    )


def _table_needs_archival(
    sync_conn,
    table_name: str,
    expected_columns: set[str],
    uuid_columns: set[str],
) -> bool:
    existing_tables = set(inspect(sync_conn).get_table_names())
    if table_name not in existing_tables:
        return False

    columns = _load_columns(sync_conn, table_name)
    if not expected_columns.issubset(columns):
        return True

    for column_name in uuid_columns:
        column_type = str(columns[column_name]["type"]).lower()
        if "uuid" not in column_type:
            return True

    return False


def _prepare_schema_compatibility(sync_conn) -> None:
    if _table_needs_archival(
        sync_conn,
        table_name="import_job_items",
        expected_columns=CURRENT_IMPORT_JOB_ITEMS_COLUMNS,
        uuid_columns={"id", "job_id"},
    ):
        _archive_table(sync_conn, "import_job_items")

    if _table_needs_archival(
        sync_conn,
        table_name="import_jobs",
        expected_columns=CURRENT_IMPORT_JOBS_COLUMNS,
        uuid_columns={"id"},
    ):
        _archive_table(sync_conn, "import_jobs")


def _split_sql_statements(sql: str) -> list[str]:
    # Keep imports-core migration files to simple semicolon-terminated DDL.
    # This splitter does not parse function bodies or string literals.
    return [statement.strip() for statement in sql.split(";") if statement.strip()]


async def run_sql_migrations() -> None:
    migration_files = sorted(MIGRATIONS_DIR.glob("*.sql"))
    if not migration_files:
        logger.warning("No SQL migrations found in %s", MIGRATIONS_DIR)
        return

    try:
        async with engine.begin() as conn:
            await conn.execute(text(f"SELECT pg_advisory_xact_lock({MIGRATION_LOCK_KEY})"))
            await conn.run_sync(_prepare_schema_compatibility)
            for migration_file in migration_files:
                sql = migration_file.read_text(encoding="utf-8")
                logger.info("Applying SQL migration %s", migration_file.name)
                for statement in _split_sql_statements(sql):
                    await conn.execute(text(statement))
    except Exception as exc:
        logger.exception("Failed while applying imports-core SQL migrations")
        raise RuntimeError("imports-core startup migrations failed") from exc


async def get_db_session() -> AsyncGenerator[AsyncSession, None]:
    async with SessionLocal() as session:
        yield session
