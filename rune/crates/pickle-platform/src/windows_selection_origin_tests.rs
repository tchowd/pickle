//! Pure tests of the same origin-proof/read/publication ordering used by UIA.
//! They model stale provider observations, not Windows native delivery evidence.
use super::*;

struct Reader {
    parents: Vec<Option<usize>>,
    roots: Vec<usize>,
    focuses: Vec<usize>,
    root_reads: usize,
    focus_reads: usize,
    password_nodes: Vec<usize>,
    password_error: Option<usize>,
    parent_error: Option<usize>,
    compare_error: bool,
    reparent_after_read: Option<(usize, Option<usize>)>,
    native_change_after_read: bool,
    validations: usize,
    password_checks: usize,
    parent_reads: usize,
    pattern_requests: usize,
    selection_reads: usize,
    text_reads: usize,
}
impl Default for Reader {
    fn default() -> Self {
        Self {
            // A and B share process identity. Only the window-root identity
            // differs: focused A control 2 -> virtual layout 4 -> root A 0;
            // stale B control 3 -> root B 1 -> desktop 5.
            parents: vec![Some(5), Some(5), Some(4), Some(1), Some(0), None],
            roots: vec![0],
            focuses: vec![2],
            root_reads: 0,
            focus_reads: 0,
            password_nodes: vec![],
            password_error: None,
            parent_error: None,
            compare_error: false,
            reparent_after_read: None,
            native_change_after_read: false,
            validations: 0,
            password_checks: 0,
            parent_reads: 0,
            pattern_requests: 0,
            selection_reads: 0,
            text_reads: 0,
        }
    }
}
impl Reader {
    fn assert_no_text_access(&self) {
        assert_eq!(
            (self.pattern_requests, self.selection_reads, self.text_reads),
            (0, 0, 0)
        );
    }
}
impl SelectionReader for Reader {
    type Node = usize;
    fn validate_source(&mut self) -> Result<(), PlatformError> {
        self.validations += 1;
        if self.native_change_after_read && self.text_reads > 0 {
            return Err(origin_error());
        }
        Ok(())
    }
    fn root(&mut self) -> Result<usize, PlatformError> {
        let root = *self
            .roots
            .get(self.root_reads)
            .unwrap_or_else(|| self.roots.last().unwrap());
        self.root_reads += 1;
        Ok(root)
    }
    fn focused(&mut self) -> Result<usize, PlatformError> {
        let focus = *self
            .focuses
            .get(self.focus_reads)
            .unwrap_or_else(|| self.focuses.last().unwrap());
        self.focus_reads += 1;
        Ok(focus)
    }
    fn same(&mut self, left: &usize, right: &usize) -> Result<bool, PlatformError> {
        if self.compare_error {
            return Err(origin_error());
        }
        Ok(left == right)
    }
    fn password(&mut self, node: &usize) -> Result<bool, PlatformError> {
        self.password_checks += 1;
        if self.password_error == Some(*node) {
            return Err(origin_error());
        }
        Ok(self.password_nodes.contains(node))
    }
    fn parent(&mut self, node: &usize) -> Result<usize, PlatformError> {
        self.parent_reads += 1;
        if self.parent_error == Some(*node) {
            return Err(origin_error());
        }
        self.parents[*node].ok_or_else(origin_error)
    }
    fn selected_text(&mut self, _: &usize) -> Result<String, PlatformError> {
        self.pattern_requests += 1;
        self.selection_reads += 1;
        self.text_reads += 1;
        if let Some((node, parent)) = self.reparent_after_read {
            self.parents[node] = parent;
        }
        Ok("Café selected in A".into())
    }
}

#[test]
fn same_pid_stale_other_window_focus_is_refused_before_any_text_pattern_access() {
    let mut reader = Reader {
        focuses: vec![3],
        ..Reader::default()
    };
    assert!(read_bound_selection(&mut reader).is_err());
    reader.assert_no_text_access();
    assert_eq!(reader.root_reads, 1);
}

#[test]
fn virtual_control_and_layout_reaching_exact_window_root_are_supported() {
    let mut reader = Reader::default();
    assert_eq!(
        read_bound_selection(&mut reader).unwrap(),
        "Café selected in A"
    );
    assert_eq!(
        (
            reader.pattern_requests,
            reader.selection_reads,
            reader.text_reads
        ),
        (1, 1, 1)
    );
    assert_eq!(reader.validations, 3);
    assert_eq!(reader.root_reads, 2);
    assert_eq!(reader.focus_reads, 2);
}

#[test]
fn password_leaf_virtual_ancestor_and_window_root_each_block_before_text() {
    for node in [2, 4, 0] {
        let mut reader = Reader {
            password_nodes: vec![node],
            ..Reader::default()
        };
        assert!(read_bound_selection(&mut reader).is_err());
        reader.assert_no_text_access();
    }
}

#[test]
fn unknown_password_failed_parent_null_parent_and_compare_error_fail_closed() {
    let mut cases = vec![
        Reader {
            password_error: Some(4),
            ..Reader::default()
        },
        Reader {
            parent_error: Some(4),
            ..Reader::default()
        },
        Reader {
            compare_error: true,
            ..Reader::default()
        },
    ];
    let mut ended = Reader::default();
    ended.parents[4] = None;
    cases.push(ended);
    for mut reader in cases {
        assert!(read_bound_selection(&mut reader).is_err());
        reader.assert_no_text_access();
    }
}

#[test]
fn cyclic_or_overdeep_provider_tree_cannot_read_text_or_loop_forever() {
    let mut cyclic = Reader::default();
    cyclic.parents[4] = Some(2);
    assert!(read_bound_selection(&mut cyclic).is_err());
    cyclic.assert_no_text_access();
    assert_eq!(cyclic.password_checks, SELECTION_ANCESTRY_LIMIT);
    assert_eq!(cyclic.parent_reads, SELECTION_ANCESTRY_LIMIT - 1);
    for nodes in [SELECTION_ANCESTRY_LIMIT, SELECTION_ANCESTRY_LIMIT + 1] {
        let mut reader = Reader {
            parents: (0..nodes).map(|n| n.checked_sub(1)).collect(),
            focuses: vec![nodes - 1],
            ..Reader::default()
        };
        let result = read_bound_selection(&mut reader);
        assert_eq!(result.is_ok(), nodes == SELECTION_ANCESTRY_LIMIT);
        if result.is_err() {
            reader.assert_no_text_access();
        }
    }
}

#[test]
fn changed_root_focus_native_identity_or_raw_parent_discards_read_text_before_publication() {
    for mut reader in [
        Reader {
            roots: vec![0, 1],
            ..Reader::default()
        },
        Reader {
            focuses: vec![2, 3],
            ..Reader::default()
        },
        Reader {
            native_change_after_read: true,
            ..Reader::default()
        },
        Reader {
            reparent_after_read: Some((4, Some(1))),
            ..Reader::default()
        },
    ] {
        assert!(read_bound_selection(&mut reader).is_err());
        assert_eq!(reader.text_reads, 1);
    }
}

#[test]
fn request_timeout_is_clipped_to_remaining_budget_and_expiry_is_not_a_one_ms_extension() {
    let now = Instant::now();
    assert_eq!(
        selection_request_ms(now + Duration::from_secs(2), now).unwrap(),
        500
    );
    assert_eq!(
        selection_request_ms(now + Duration::from_millis(17), now).unwrap(),
        17
    );
    assert_eq!(
        selection_request_ms(now + Duration::from_micros(1), now).unwrap(),
        1
    );
    assert!(selection_request_ms(now, now).is_err());
    assert!(selection_request_ms(now, now + Duration::from_millis(1)).is_err());
}
