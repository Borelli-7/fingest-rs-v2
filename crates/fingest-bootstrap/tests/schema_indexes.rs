//! Schema guards that apply across every bounded context.
//!
//! Lives in the composition root because this is the crate that runs the migrations.

use sqlx::PgPool;

/// Postgres does not index the referencing side of a foreign key. Without one, deleting or
/// renaming the referenced row, and every lookup by that column, scans the whole table.
/// Any future foreign key must arrive with an index that leads with its columns.
#[sqlx::test(migrations = "../../migrations")]
async fn every_foreign_key_is_covered_by_a_leading_index(pool: PgPool) {
    let foreign_keys: Vec<(String, String, bool)> = sqlx::query_as(
        r#"
        SELECT c.conrelid::regclass::text,
               c.conname::text,
               EXISTS (
                   SELECT 1 FROM pg_index i
                   WHERE i.indrelid = c.conrelid
                     AND (i.indkey::int2[])[0:cardinality(c.conkey) - 1] @> c.conkey
                     AND (i.indkey::int2[])[0:cardinality(c.conkey) - 1] <@ c.conkey
               )
        FROM pg_constraint c
        WHERE c.contype = 'f'
          AND c.connamespace = 'public'::regnamespace
        ORDER BY 1, 2
        "#,
    )
    .fetch_all(&pool)
    .await
    .unwrap();

    // Guards against the query silently matching nothing.
    let tables: Vec<&str> = foreign_keys.iter().map(|(t, _, _)| t.as_str()).collect();
    for table in ["account_wallet", "budget", "expense", "saving"] {
        assert!(tables.contains(&table), "no foreign key found on {table}");
    }

    let uncovered: Vec<String> = foreign_keys
        .iter()
        .filter(|(_, _, covered)| !covered)
        .map(|(table, name, _)| format!("{table}.{name}"))
        .collect();
    assert!(
        uncovered.is_empty(),
        "foreign keys without a leading index: {uncovered:?}"
    );
}

async fn plan(pool: &PgPool, sql: &str) -> String {
    let mut conn = pool.acquire().await.unwrap();
    // The test tables are tiny, so the planner would rightly pick a sequential scan; this
    // asks only whether an index *can* serve the query shape.
    sqlx::query("SET enable_seqscan = off")
        .execute(&mut *conn)
        .await
        .unwrap();
    let lines: Vec<String> = sqlx::query_scalar(&format!("EXPLAIN {sql}"))
        .fetch_all(&mut *conn)
        .await
        .unwrap();
    lines.join("\n")
}

/// The wallet expense listing filters on wallet and date together.
#[sqlx::test(migrations = "../../migrations")]
async fn wallet_expense_listing_can_use_an_index(pool: PgPool) {
    let plan = plan(
        &pool,
        "SELECT id FROM expense WHERE wallet_id = 1 \
         AND date BETWEEN DATE '2024-01-01' AND DATE '2024-12-31' ORDER BY date, id",
    )
    .await;

    assert!(plan.contains("idx_expense_wallet_date"), "{plan}");
}

/// Budget listing starts from the owner's budgets.
#[sqlx::test(migrations = "../../migrations")]
async fn budget_listing_by_owner_can_use_an_index(pool: PgPool) {
    let plan = plan(&pool, "SELECT id FROM budget WHERE account_login = 'user1'").await;

    assert!(plan.contains("idx_budget_account"), "{plan}");
}
