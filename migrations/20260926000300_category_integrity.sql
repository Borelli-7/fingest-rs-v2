-- 1. Renaming a category that expenses or budgets reference failed on the foreign key, so
--    only unused categories could be renamed. Let renames propagate.
ALTER TABLE expense
    DROP CONSTRAINT IF EXISTS expense_category_name_category_profit_fkey,
    ADD CONSTRAINT expense_category_name_category_profit_fkey
        FOREIGN KEY (category_name, category_profit)
        REFERENCES category (name, profit) ON UPDATE CASCADE;

ALTER TABLE budget
    DROP CONSTRAINT IF EXISTS budget_category_name_category_profit_fkey,
    ADD CONSTRAINT budget_category_name_category_profit_fkey
        FOREIGN KEY (category_name, category_profit)
        REFERENCES category (name, profit) ON UPDATE CASCADE;

-- 2. Case-insensitive uniqueness was only checked in application code, so two concurrent
--    creates of "Food" and "FOOD" could both succeed. Fold any such duplicates into the
--    alphabetically first spelling, then let the database enforce it.
CREATE TEMPORARY TABLE category_merge ON COMMIT DROP AS
SELECT name, profit, keep
FROM (
    SELECT name, profit, MIN(name) OVER (PARTITION BY LOWER(name), profit) AS keep
    FROM category
) ranked
WHERE name <> keep;

UPDATE expense e
SET category_name = m.keep
FROM category_merge m
WHERE e.category_name = m.name AND e.category_profit = m.profit;

UPDATE budget b
SET category_name = m.keep
FROM category_merge m
WHERE b.category_name = m.name AND b.category_profit = m.profit;

DELETE FROM category c
USING category_merge m
WHERE c.name = m.name AND c.profit = m.profit;

CREATE UNIQUE INDEX IF NOT EXISTS category_name_ci_unique ON category (LOWER(name), profit);
