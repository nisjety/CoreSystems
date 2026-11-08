import json, importlib.util, sys


def load(name):
    spec = importlib.util.spec_from_file_location(name, name + '.py')
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


m1, m2, m3 = load('s8b_part1'), load('s8b_part2'), load('s8b_part3')
spec = {'slice': m1.SLICE, 'summary': m1.SUMMARY, 'approach': m1.APPROACH, 'file_changes': m2.FILE_CHANGES,
        'interfaces': m2.INTERFACES, 'tests': m2.TESTS, 'steps': m3.STEPS, 'risks': m3.RISKS,
        'invariants': m3.INVARIANTS, 'failure_matrix': m3.FAILURE_MATRIX,
        'amendments_to_prior_specs': m3.AMENDMENTS, 'open_questions': m3.OPEN_QUESTIONS,
        'verification_commands': m3.VERIFICATION_COMMANDS}

patched = sys.argv[2:] != ['--no-rev5']
if patched:
    r5 = load('s8b_rev5')
    r5.apply(spec)
    print('rev5 replacements:', len(r5.LOG))
    r6 = load('s8b_rev6')
    r6.apply(spec)
    print('rev5+rev6 replacements:', len(r6.LOG))
    r7 = load('s8b_rev7')
    r7.apply(spec)
    print('rev7 replacements:', len(r7.LOG))
    r8 = load('s8b_rev8')
    r8.apply(spec)
    print('rev8 replacements:', len(r8.LOG))

out = sys.argv[1]
json.dump(spec, open(out, 'w', encoding='utf-8'), ensure_ascii=False, indent=1)
txt = json.dumps(spec, ensure_ascii=False)
stale = ['mark_extensions', 'static_parkable', 'retire_before', 'three places', 'fifth bind', '1800',
         'three consecutive', 'retire(fold, reason)', 'EXT_PARKABLE', 'crash_marked_in',
         'three times', 'three attempts', '6.4 s', '6 s of retries', 'three transient', 'fail open', 'fail-open',
         'Denied | NotFound', 'Denied or NotFound', 'Stale or Closed)', 'last_beat.elapsed', 'crash_park_db_tests',
         'after the S8-A spare', 'spare_parked_run again', 'retried twice', 'a_read_only_rung_refuses',
         'Instant::now() taken', 'heartbeat_floor: std::time::Instant', 'cargo fmt --all -- --check (',
         'dfd1a8d5 as the S8-B base', 'diff --stat HEAD']
for bad in stale:
    print(repr(bad), txt.count(bad))
print(len(txt))
