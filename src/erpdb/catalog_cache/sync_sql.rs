pub(super) const ITEMS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_name, '') AS item_name,
        COALESCE(stock_uom, '') AS stock_uom,
        COALESCE(item_group, '') AS item_group,
        COALESCE(CAST(modified AS CHAR), '') AS modified,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(is_stock_item, 0) AS is_stock_item
    FROM tabItem
"#;

pub(super) const CHANGED_ITEMS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_name, '') AS item_name,
        COALESCE(stock_uom, '') AS stock_uom,
        COALESCE(item_group, '') AS item_group,
        COALESCE(CAST(modified AS CHAR), '') AS modified,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(is_stock_item, 0) AS is_stock_item
    FROM tabItem
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const ITEM_GROUPS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_group_name, '') AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group,
        COALESCE(lft, 0) AS lft,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Group`
"#;

pub(super) const CHANGED_ITEM_GROUPS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(item_group_name, '') AS item_group_name,
        COALESCE(parent_item_group, '') AS parent_item_group,
        COALESCE(is_group, 0) AS is_group,
        COALESCE(lft, 0) AS lft,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Group`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const SUPPLIERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(supplier_name, '') AS supplier_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(supplier_details, '') AS supplier_details,
        COALESCE(image, '') AS image,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabSupplier
"#;

pub(super) const CHANGED_SUPPLIERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(supplier_name, '') AS supplier_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(supplier_details, '') AS supplier_details,
        COALESCE(image, '') AS image,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabSupplier
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const CUSTOMERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(customer_name, '') AS customer_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(customer_details, '') AS customer_details,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabCustomer
"#;

pub(super) const CHANGED_CUSTOMERS_SQL: &str = r#"
    SELECT
        name,
        COALESCE(customer_name, '') AS customer_name,
        COALESCE(mobile_no, '') AS mobile_no,
        COALESCE(customer_details, '') AS customer_details,
        COALESCE(disabled, 0) AS disabled,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM tabCustomer
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const ITEM_SUPPLIERS_SQL: &str = r#"
    SELECT
        parent,
        supplier,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Supplier`
"#;

pub(super) const CHANGED_ITEM_SUPPLIERS_SQL: &str = r#"
    SELECT
        parent,
        supplier,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Supplier`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const ITEM_CUSTOMERS_SQL: &str = r#"
    SELECT
        parent,
        customer_name,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Customer Detail`
"#;

pub(super) const CHANGED_ITEM_CUSTOMERS_SQL: &str = r#"
    SELECT
        parent,
        customer_name,
        COALESCE(CAST(modified AS CHAR), '') AS modified
    FROM `tabItem Customer Detail`
    WHERE COALESCE(CAST(modified AS CHAR), '') > ?
"#;

pub(super) const ITEM_KEYS_SQL: &str = "SELECT name FROM tabItem";
pub(super) const ITEM_GROUP_KEYS_SQL: &str = "SELECT name FROM `tabItem Group`";
pub(super) const SUPPLIER_KEYS_SQL: &str = "SELECT name FROM tabSupplier";
pub(super) const CUSTOMER_KEYS_SQL: &str = "SELECT name FROM tabCustomer";
pub(super) const ITEM_SUPPLIER_KEYS_SQL: &str =
    "SELECT parent AS left_key, supplier AS right_key FROM `tabItem Supplier`";
pub(super) const ITEM_CUSTOMER_KEYS_SQL: &str =
    "SELECT parent AS left_key, customer_name AS right_key FROM `tabItem Customer Detail`";

pub(super) const ITEM_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabItem";
pub(super) const ITEM_GROUP_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Group`";
pub(super) const SUPPLIER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabSupplier";
pub(super) const CUSTOMER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM tabCustomer";
pub(super) const ITEM_SUPPLIER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Supplier`";
pub(super) const ITEM_CUSTOMER_STATS_SQL: &str = "SELECT COUNT(*) AS row_count, COALESCE(MAX(CAST(modified AS CHAR)), '') AS max_modified FROM `tabItem Customer Detail`";
