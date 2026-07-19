#[cfg(test)]
mod tests {
    use crate::db::categories::{self, CategoriesRow};
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        crate::db::test_helpers::setup_test_db()
    }

    #[test]
    fn test_categories_crud() {
        let conn = setup_db();

        let cat_parent = CategoriesRow {
            id: "cat_parent".into(),
            name: "Custom Parent".into(),
            parent_id: None,
            source_type: "user".into(),
            mcc_code: None,
            monthly_budget_minor: None,
            is_deleted: false,
            created_at: None,
            color: None,
            icon: None,
        };

        categories::insert(&conn, &cat_parent).unwrap();

        let cat_child = CategoriesRow {
            id: "cat_child".into(),
            name: "Custom Child".into(),
            parent_id: Some("cat_parent".into()),
            source_type: "user".into(),
            mcc_code: None,
            monthly_budget_minor: None,
            is_deleted: false,
            created_at: None,
            color: None,
            icon: None,
        };

        categories::insert(&conn, &cat_child).unwrap();

        // test select by id
        let fetched_parent = categories::select_by_id(&conn, "cat_parent")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_parent.name, "Custom Parent");

        let fetched_child = categories::select_by_id(&conn, "cat_child")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_child.parent_id.as_deref(), Some("cat_parent"));

        // test update
        let mut updated_child = fetched_child.clone();
        updated_child.name = "Updated Child".into();
        categories::update(&conn, &updated_child).unwrap();

        let fetched_updated_child = categories::select_by_id(&conn, "cat_child")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_updated_child.name, "Updated Child");

        // test soft delete
        categories::soft_delete(&conn, "cat_child", false).unwrap();
        assert!(categories::select_by_id(&conn, "cat_child")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_system_category_seeding() {
        let conn = setup_db();

        let categories = categories::select_all(&conn).unwrap();
        assert!(
            !categories.is_empty(),
            "Seeds should populate system categories"
        );

        let food_cat = categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .unwrap();
        assert_eq!(food_cat.source_type, "system");

        let groceries_cat = categories.iter().find(|c| c.name == "Groceries").unwrap();
        assert_eq!(groceries_cat.source_type, "system");
        assert_eq!(groceries_cat.parent_id.as_deref(), Some("cat_food"));
    }
    #[test]
    fn test_system_category_protection() {
        let conn = setup_db();

        let categories = categories::select_all(&conn).unwrap();
        let food_cat = categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .unwrap();

        // Attempt to soft delete
        let delete_res = categories::soft_delete(&conn, &food_cat.id, false);
        assert!(
            delete_res.is_err(),
            "Should not be able to delete a system category"
        );

        // Attempt to rename
        let mut modified = food_cat.clone();
        modified.name = "Renamed Food".into();
        let update_res = categories::update(&conn, &modified);
        assert!(
            update_res.is_err(),
            "Should not be able to rename a system category"
        );

        // Attempt to reparent
        let mut reparented = food_cat.clone();
        reparented.parent_id = Some("some_other_id".into());
        let reparent_res = categories::update(&conn, &reparented);
        assert!(
            reparent_res.is_err(),
            "Should not be able to reparent a system category"
        );

        // Attempt to set a monthly budget (should be allowed -- name/parent_id
        // are the only protected fields on a system category)
        let mut budgeted = food_cat.clone();
        budgeted.monthly_budget_minor = Some(500000);
        let budget_res = categories::update(&conn, &budgeted);
        assert!(
            budget_res.is_ok(),
            "Should be able to set a budget on a system category"
        );
    }

    /// Doc 30 TASK-API-007 acceptance test.
    #[test]
    fn test_system_category_delete_rejected() {
        let conn = setup_db();
        let categories = categories::select_all(&conn).unwrap();
        let food_cat = categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .unwrap();

        let result = categories::soft_delete(&conn, &food_cat.id, false);
        assert!(result.is_err(), "a system category must never be deletable");
    }

    /// Doc 30 TASK-API-007 acceptance test: "icon/color customization is
    /// allowed" on `categories_update`, even for a system category whose
    /// name/parent_id are protected -- exercises migration
    /// 20260101000039's `color`/`icon` columns, added specifically to
    /// support this criterion (Document 18 §4.9's original schema had
    /// neither column at all).
    #[test]
    fn test_system_category_icon_update_allowed() {
        let conn = setup_db();
        let categories = categories::select_all(&conn).unwrap();
        let food_cat = categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .unwrap();

        let mut restyled = food_cat.clone();
        restyled.icon = Some("new-icon".to_string());
        restyled.color = Some("#123456".to_string());
        categories::update(&conn, &restyled).unwrap();

        let refetched = categories::select_by_id(&conn, &food_cat.id)
            .unwrap()
            .unwrap();
        assert_eq!(refetched.icon.as_deref(), Some("new-icon"));
        assert_eq!(refetched.color.as_deref(), Some("#123456"));
        assert_eq!(
            refetched.name, "Food & Dining",
            "name must remain untouched by an icon/color-only update"
        );
    }

    /// Doc 30 TASK-API-007 acceptance test: deleting a user category with
    /// linked transactions reassigns them to the seeded `cat_others`
    /// system category (migration 20260101000040) rather than leaving a
    /// dangling `category_id` or silently refusing the delete outright.
    #[test]
    fn test_category_delete_reassigns_transactions() {
        let conn = setup_db();
        conn.execute("PRAGMA foreign_keys = OFF;", []).unwrap();

        let user_cat = CategoriesRow {
            id: "cat_user_custom".into(),
            name: "My Custom Category".into(),
            parent_id: None,
            source_type: "user".into(),
            mcc_code: None,
            monthly_budget_minor: None,
            is_deleted: false,
            created_at: None,
            color: None,
            icon: None,
        };
        categories::insert(&conn, &user_cat).unwrap();
        conn.execute(
            "INSERT INTO transactions (id, category_id, is_deleted) VALUES ('tx_1', 'cat_user_custom', 0)",
            [],
        )
        .unwrap();

        // Without confirmation, the delete is rejected rather than silently
        // reassigning.
        let unconfirmed = categories::soft_delete(&conn, "cat_user_custom", false);
        assert!(
            unconfirmed.is_err(),
            "must require confirm_reassign when transactions are linked"
        );

        let reassigned_count = categories::soft_delete(&conn, "cat_user_custom", true).unwrap();
        assert_eq!(reassigned_count, 1);

        let category_id: String = conn
            .query_row(
                "SELECT category_id FROM transactions WHERE id = 'tx_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(category_id, "cat_others");

        assert!(categories::select_by_id(&conn, "cat_user_custom")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_hierarchy_traversal() {
        let conn = setup_db();
        let categories = categories::select_all(&conn).unwrap();

        // Find food & dining
        let food_cat = categories
            .iter()
            .find(|c| c.name == "Food & Dining")
            .unwrap();

        // Find all children of food & dining
        let food_children: Vec<_> = categories
            .iter()
            .filter(|c| c.parent_id.as_deref() == Some(&food_cat.id))
            .collect();

        assert!(!food_children.is_empty());
        assert!(food_children.iter().any(|c| c.name == "Groceries"));
        assert!(food_children.iter().any(|c| c.name == "Restaurants"));
    }
}
