// Integration tests for Phase 2.1 (Table Indexing)
// Tests using indexed table operations

#[cfg(test)]
mod phase_2_1_tests {
    use crate::runtime::table::Table;
    use interstice_abi::{Row, IntersticeValue, schema::{TableSchema, TableVisibility, EntrySchema}, IntersticeType};

    fn create_test_schema() -> TableSchema {
        TableSchema {
            name: "test_table".to_string(),
            visibility: TableVisibility::Public,
            primary_key: EntrySchema {
                name: "id".to_string(),
                value_type: IntersticeType::U64,
            },
            entries: vec![
                EntrySchema {
                    name: "email".to_string(),
                    value_type: IntersticeType::String,
                },
                EntrySchema {
                    name: "age".to_string(),
                    value_type: IntersticeType::U32,
                },
            ],
        }
    }

    fn create_test_row(id: u64, email: &str, age: u32) -> Row {
        Row {
            primary_key: IntersticeValue::U64(id),
            entries: vec![
                IntersticeValue::String(email.to_string()),
                IntersticeValue::U32(age),
            ],
        }
    }

    #[test]
    fn test_table_creation_with_schema() {
        let schema = create_test_schema();
        let table = Table::new(schema.clone());
        assert_eq!(table.len(), 0);
        assert!(table.is_empty());
        assert_eq!(table.schema.name, "test_table");
    }

    #[test]
    fn test_table_insert_row() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        let row = create_test_row(1, "user@example.com", 30);
        
        let row_idx = table.insert(row).expect("Insert failed");
        assert_eq!(row_idx, 0);
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn test_table_get_row() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        let row = create_test_row(1, "user@example.com", 30);
        
        table.insert(row.clone()).expect("Insert failed");
        let retrieved = table.get(0).expect("Get failed");
        assert_eq!(retrieved.primary_key, row.primary_key);
    }

    #[test]
    fn test_table_find_by_primary_key() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        let pk = IntersticeValue::U64(1);
        let row = create_test_row(1, "user@example.com", 30);
        
        table.insert(row).expect("Insert failed");
        
        let found = table.find_by_pk(&pk);
        assert!(found.is_some());
        let (idx, retrieved_row) = found.unwrap();
        assert_eq!(idx, 0);
        assert_eq!(retrieved_row.primary_key, pk);
    }

    #[test]
    fn test_table_secondary_index_creation() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        // Create index on email column
        table.create_index("email").expect("Index creation failed");
        
        // Insert rows
        let row1 = create_test_row(1, "alice@example.com", 30);
        let row2 = create_test_row(2, "bob@example.com", 25);
        table.insert(row1).expect("Insert failed");
        table.insert(row2).expect("Insert failed");
    }

    #[test]
    fn test_table_query_by_secondary_index() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        table.create_index("email").expect("Index creation failed");
        
        let row = create_test_row(1, "user@example.com", 30);
        table.insert(row).expect("Insert failed");
        
        let email_value = IntersticeValue::String("user@example.com".to_string());
        let results = table.query_by_index("email", &email_value);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_table_iter() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=5 {
            let row = create_test_row(i, &format!("user{}@example.com", i), 25 + i as u32);
            table.insert(row).expect("Insert failed");
        }
        
        let count: usize = table.iter().count();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_table_invalid_row_rejected() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        // Create a row with wrong number of entries
        let invalid_row = Row {
            primary_key: IntersticeValue::U64(1),
            entries: vec![IntersticeValue::String("test@example.com".to_string())], // Missing age
        };
        
        let result = table.insert(invalid_row);
        assert!(result.is_err());
    }

    #[test]
    fn test_table_multiple_inserts_and_lookup() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        for i in 1..=10 {
            let row = create_test_row(i, &format!("user{}@example.com", i), 20 + (i as u32 % 40));
            table.insert(row).expect("Insert failed");
        }
        
        assert_eq!(table.len(), 10);
        
        // Lookup specific row
        let pk = IntersticeValue::U64(5);
        let found = table.find_by_pk(&pk);
        assert!(found.is_some());
        let (_, row) = found.unwrap();
        assert_eq!(row.primary_key, pk);
    }

    #[test]
    fn test_table_index_on_nonexistent_column_fails() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        let result = table.create_index("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_table_duplicate_index_fails() {
        let schema = create_test_schema();
        let mut table = Table::new(schema);
        
        table.create_index("email").expect("First index creation failed");
        let result = table.create_index("email");
        assert!(result.is_err());
    }
}
