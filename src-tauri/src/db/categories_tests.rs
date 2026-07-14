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
            color: Some("#000000".into()),
            icon: Some("box".into()),
            is_system: false,
            is_deleted: false,
            created_at: None,
            updated_at: None,
        };

        categories::insert(&conn, &cat_parent).unwrap();

        let cat_child = CategoriesRow {
            id: "cat_child".into(),
            name: "Custom Child".into(),
            parent_id: Some("cat_parent".into()),
            color: None,
            icon: None,
            is_system: false,
            is_deleted: false,
            created_at: None,
            updated_at: None,
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
        categories::soft_delete(&conn, "cat_child").unwrap();
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
        assert!(food_cat.is_system);

        let groceries_cat = categories.iter().find(|c| c.name == "Groceries").unwrap();
        assert!(groceries_cat.is_system);
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
        let delete_res = categories::soft_delete(&conn, &food_cat.id);
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

        // Attempt to change color (should be allowed)
        let mut recolored = food_cat.clone();
        recolored.color = Some("#000000".into());
        let recolor_res = categories::update(&conn, &recolored);
        assert!(
            recolor_res.is_ok(),
            "Should be able to recolor a system category"
        );
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
