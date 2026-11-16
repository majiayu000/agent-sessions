//! Checked SQLite sizes remain fallible after the rusqlite dependency update.
#[test]
fn sqlite_length_reads_reject_negative_and_out_of_range_values() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    for value in [0_i64, i64::MAX] {
        assert_eq!(
            conn.query_row("SELECT ?1", [value], |row| row.get::<_, u64>(0))
                .unwrap(),
            value as u64,
        );
    }
    assert!(matches!(
        conn.query_row("SELECT -1", [], |row| row.get::<_, u64>(0)),
        Err(rusqlite::Error::IntegralValueOutOfRange(0, -1))
    ));
    assert!(matches!(
        conn.query_row("SELECT 9223372036854775808", [], |row| row.get::<_, u64>(0)),
        Err(rusqlite::Error::InvalidColumnType(..))
    ));
    assert_eq!(
        conn.query_row("SELECT NULL", [], |row| row.get::<_, Option<u64>>(0))
            .unwrap(),
        None
    );
    assert!(
        conn.query_row("SELECT -1", [], |row| row.get::<_, Option<u64>>(0))
            .is_err()
    );
}
