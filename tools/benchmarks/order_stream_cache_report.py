#!/usr/bin/env python3
"""Render the modern-protocol snapshot-cache comparison from sanitized results."""
import argparse
import json
from pathlib import Path


def load(root, name):
    return json.loads((root / name).read_text())


def phase(result, name):
    return next(row for row in result['phases'] if row['name'] == name)


def pct(before, after):
    return (after / before - 1) * 100


def matching_rows(left, right):
    assert [row['name'] for row in left] == [row['name'] for row in right], (
        'Missing, extra or differently ordered comparison rows'
    )
    return zip(left, right)


def validate(result, expected_interval):
    assert result['protocols'] == 'modern' and result['include_control'] is True
    assert result['server']['mini_order_sync_interval_seconds'] == expected_interval
    for row in result['correctness']:
        assert row.get('exact_rest_view_match', row.get('exact_rest_page_match'))
    for group in result['fanout']:
        assert len(group['records']) == 2 * group['cycles']
        assert [row['action'] for row in group['records']] == ['hold', 'failed'] * group['cycles']
        assert group['control_returned'] == sum(
            row['post']['control_returned'] for row in group['records']
        )
        assert group['control_returned'] + group['control_null'] == len(group['records'])
        for record in group['records']:
            assert all(event['exact_target_transition'] for event in record['events'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-dir', required=True)
    parser.add_argument('--output-dir', required=True)
    parser.add_argument('--date', required=True)
    args = parser.parse_args()
    root, out = Path(args.run_dir), Path(args.output_dir)
    primary = {m: load(root, m + '-results.json') for m in ('before', 'after')}
    repeat = {m: load(root, m + '-reverse-repeat.json') for m in primary}
    sql = {m: load(root, m + '-sql-counts.json') for m in primary}
    cross = load(root, 'crossprocess-results.json')
    cleanup = load(root, 'cleanup.json')
    verification = load(root, 'verification.json')
    intermediate_path = root / 'intermediate-460f1d54-after-results.json'
    intermediate = load(root, intermediate_path.name) if intermediate_path.exists() else None
    before, after = primary['before'], primary['after']
    if intermediate:
        assert intermediate['server']['source_revision'].startswith('460f1d54')
        assert intermediate['dataset_dump_sha256'] == before['dataset_dump_sha256']
        assert intermediate['selection_fingerprints'] == before['selection_fingerprints']
    assert cleanup['benchmark_servers_stopped']
    for group, interval in ((primary, 30), (repeat, 30), (sql, 0)):
        for rows in ('phases', 'fanout', 'admin_pages'):
            list(matching_rows(group['before'][rows], group['after'][rows]))
        for mode, result in group.items():
            validate(result, interval)
            assert result['dataset_dump_sha256'] == before['dataset_dump_sha256']
            assert result['selection_fingerprints'] == before['selection_fingerprints']
            assert result['host'] == before['host']
            assert result['harness_sha256'] == before['harness_sha256']
            assert result['server']['binary_sha256'] == primary[mode]['server']['binary_sha256']
            assert result['server']['query_log'] is (group is sql)
            assert result['command_config'] == group['before']['command_config']
        for b, a in matching_rows(group['before']['fanout'], group['after']['fanout']):
            assert b['total_frame_bytes'] == a['total_frame_bytes'], (
                'Worker payload byte count differs; investigate before publishing parity'
            )
    for result in [*primary.values(), *repeat.values()]:
        assert all(len(row['records']) >= 40 for row in result['fanout'])
    assert cross['candidate_binary_sha256'] == after['server']['binary_sha256']
    assert cross['restored_original_order'] and cross['independent_os_processes'] == 2
    assert all(result['reorder']['restored_original_order'] for result in primary.values())
    assert len(before['reorder']['records']) == len(after['reorder']['records']) == 2
    lines = [
        '# Snapshot cache reuse: measured comparison', '',
        'Date: ' + args.date + '. This compares the **previous optimized release** '
        '`c50e40c0` (reported at `98147693`) with the new cache optimization. '
        'It does not compare against original `main`.', '',
        'Both sides use modern worker/page/notification protocols, '
        '`include_control=true`, a 16-connection pool and the default 30-second '
        'reconciler during timing. No diagnostic tracing is enabled.', '',
        '## Provenance', '', '| Item | Value |', '|---|---|',
        '| Before source | `' + before['server']['source_revision'] + '` |',
        '| After source | `' + after['server']['source_revision'] + '` |',
        '| Before binary SHA-256 | `' + before['server']['binary_sha256'] + '` |',
        '| After binary SHA-256 | `' + after['server']['binary_sha256'] + '` |',
        '| Input dump SHA-256 | `' + before['dataset_dump_sha256'] + '` |',
        '| Harness SHA-256 | `' + before['harness_sha256'] + '` |',
        '| Host | %s; %s logical CPUs; %.0f GiB RAM |' % (
            before['host']['platform'], before['host']['logical_cpus'],
            before['host']['ram_bytes'] / 2**30), '',
        'Every run restores the same private dump. Actor, apparatus, order and '
        'all participating principal fingerprints match across primary, reverse '
        'and SQL runs. Original business records are unchanged. Timing order is '
        'baseline → candidate for the full primary run, then candidate → baseline '
        'for the independent status repeat. Each status condition has 20 '
        'hold/release cycles (40 events). Results are reported separately.', '',
    ]
    if intermediate:
        mixed = lambda result: next(
            row for row in result['fanout'] if row['name'] == 'shared_apparatus_workers_32'
        )
        lines += ['## Regression found before the final candidate', '',
            'The first cache-only candidate `460f1d54` reduced CPU but returned '
            'null controls in **%s/40** mixed-worker actions, versus **%s/40** '
            'on the baseline. These results are retained as an intermediate '
            'failure, not the final comparison. The existing 250 ms optional '
            'control deadline was too short for joining an in-flight shared '
            'snapshot. The final candidate allows a bounded 1-second shared '
            'cache wait while preserving authorization and revision checks. '
            'This cooperative async timeout cannot preempt synchronous CPU work '
            'and is not a hard ceiling for total POST latency. '
            'Final and reverse-repeat null counts and POST latency are shown '
            'below; the deadline tradeoff is not hidden.' % (
                mixed(intermediate)['control_null'], mixed(before)['control_null']), '']
    lines += [
        '## End-to-end status latency', '',
        'Latency starts before receiver submission and POST, ending when every '
        'client applies the exact generated hold or its matching release. It '
        'includes Python scheduling/reducer work. Raw socket completion, decode '
        'and reducer timings are retained in the JSON. Nearest-rank p95 of 40 '
        'samples is the third-largest sample; these are local measurements, '
        'not a production service-level guarantee.', '',
        '| Run / sockets / principals | Before median ms | After median ms | '
        'Before p95 ms | After p95 ms | p95 change | Before max ms | After max ms |',
        '|---|---:|---:|---:|---:|---:|---:|---:|',
    ]
    for label, group in [('Primary', primary), ('Reverse repeat', repeat)]:
        for b, a in matching_rows(group['before']['fanout'], group['after']['fanout']):
            assert b['name'] == a['name']
            x, y = b['all_clients_latency_ms'], a['all_clients_latency_ms']
            lines.append('| %s / %s / %s | %.2f | %.2f | %.2f | %.2f | %+.1f%% | %.2f | %.2f |' % (
                label, b['clients'], b['distinct_principals'], x['p50'], y['p50'],
                x['p95'], y['p95'], pct(x['p95'], y['p95']), x['max'], y['max']))
    mixed_p95 = []
    for group in (primary, repeat):
        values = [
            next(row for row in group[mode]['fanout']
                 if row['name'] == 'shared_apparatus_workers_32')['all_clients_latency_ms']['p95']
            for mode in ('before', 'after')
        ]
        mixed_p95.append(pct(*values))
    lines += ['', '**Mixed-worker p95 is not consistently improved:** '
        'the primary run changes by %+.1f%%, while the reverse repeat changes '
        'by %+.1f%%. CPU and control-return improvements repeat, but these '
        'measurements do not show that the remaining latency tail is solved.' % tuple(mixed_p95)]
    tail_counts = [
        sum(record['all_clients_ms'] > 700 for record in next(
            row for row in repeat[mode]['fanout']
            if row['name'] == 'shared_apparatus_workers_32')['records'])
        for mode in ('before', 'after')
    ]
    lines += ['', 'The reverse mixed-worker run has %s → %s events above '
        '700 ms. Its aggregate p95 uses the third-largest sample. A late '
        'principal group occurs in both versions; these user-space receive '
        'timestamps include client scheduling and do not prove the cause '
        'of the remaining delay.' % tuple(tail_counts)]
    lines += ['', '## Server cost and acknowledgement controls', '',
        'CPU is process CPU over the complete phase; memory is sampled RSS peak. '
        'Control latency below is the complete HTTP response arrival time, '
        'including the optional control when present; it is not an isolated '
        'server control-build timer. The harness records null controls but does '
        "not simulate Flutter's follow-up scoped bootstrap/section GETs after "
        'a null control; those costs are excluded from these POST/stream phases.', '',
        '| Run / sockets / principals | CPU ms before → after | CPU change | '
        'Peak RSS MiB before → after | POST p95 ms before → after | '
        'Control null before → after | Stream recovery GETs before → after |',
        '|---|---:|---:|---:|---:|---:|---:|']
    for label, group in [('Primary', primary), ('Reverse repeat', repeat)]:
        for b, a in matching_rows(group['before']['fanout'], group['after']['fanout']):
            x, y = phase(group['before'], b['name']), phase(group['after'], a['name'])
            gets = lambda row: sum(e.get('resync_gets', 0) for r in row['records'] for e in r['events'])
            lines.append('| %s / %s / %s | %.1f → %.1f | %+.1f%% | %.1f → %.1f | %.2f → %.2f | %s → %s | %s → %s |' % (
                label, b['clients'], b['distinct_principals'], x['server_cpu_ms'], y['server_cpu_ms'],
                pct(x['server_cpu_ms'], y['server_cpu_ms']), x['server_rss_peak_bytes']/2**20,
                y['server_rss_peak_bytes']/2**20, b['post_latency_ms']['p95'], a['post_latency_ms']['p95'],
                b['control_null'], a['control_null'], gets(b), gets(a)))
    peaks = {
        mode: max(row['server_rss_peak_bytes'] for row in result['phases']) / 2**20
        for mode, result in primary.items()
    }
    lines += ['', 'Overall primary peak RSS: %.1f → %.1f MiB (%+.1f%%). '
        'Each status condition makes 40 POSTs; the non-null control count is '
        '40 minus the null count above.' % (
            peaks['before'], peaks['after'], pct(peaks['before'], peaks['after']))]
    mixed_posts = [
        next(row for row in primary[mode]['fanout']
             if row['name'] == 'shared_apparatus_workers_32')['post_latency_ms']['p95']
        for mode in ('before', 'after')
    ]
    lines += ['', 'The primary mixed-worker POST p95 rises by %.2f ms (%+.1f%%) '
        'while null controls fall from 2/40 to 0/40. Waiting for the shared '
        'control can keep the initiating action pending longer; avoided '
        'Flutter recovery GETs are not included in these measured costs.' % (
            mixed_posts[1] - mixed_posts[0], pct(*mixed_posts))]
    lines += ['', '## Separate SQL-count diagnostic', '',
        'One hold/release cycle per status condition. Query logging and period0 '
        'are used only here. Counts include SQLx query events during each phase; '
        'their timing is not mixed with primary latency.', '',
        '| Phase | Before statements | After statements | Change |', '|---|---:|---:|---:|']
    for b, a in matching_rows(sql['before']['phases'], sql['after']['phases']):
        assert b['name'] == a['name']
        x, y = b['sqlx_statement_events'], a['sqlx_statement_events']
        lines.append('| %s | %s | %s | %+.1f%% |' % (b['name'], x, y, pct(x, y)))
    lines += ['', '## Retained behavior checks', '',
        '| Scenario | Before | After |', '|---|---:|---:|']
    for b, a in matching_rows(before['admin_pages'], after['admin_pages']):
        if b['name'].endswith('_exact_each_event'): continue
        lines.append('| %s: reload GETs / p95 ms | %s / %.2f | %s / %.2f |' % (
            b['name'], b['reload_gets'], b['latency_ms']['p95'], a['reload_gets'], a['latency_ms']['p95']))
    for i, (b, a) in enumerate(zip(before['reorder']['records'], after['reorder']['records'])):
        lines.append('| Reorder%s: delivery ms / reload GETs | %.2f / %s | %.2f / %s |' % (
            ' inverse' if i else '', b['all_views_latency_ms'], b['reload_gets'],
            a['all_views_latency_ms'], a['reload_gets']))
    for mode, result in primary.items():
        lines += ['', '**%s:** %s exact REST checks; %s stream errors; '
            'idle GETs/data events %s/%s; reconnect snapshot GETs %s; notification bytes %s.' % (
                mode.capitalize(), len(result['correctness']), len(result['stream_error_recovery']),
                result['idle']['http_gets'], result['idle']['data_events'],
                result['recovery'][0]['snapshot_gets'], result['notifications']['payload_bytes'])]
    lines += ['', '| Other primary phase | CPU ms before → after | CPU change | '
        'Peak RSS MiB before → after |', '|---|---:|---:|---:|']
    status_names = {row['name'] for row in before['fanout']}
    for b in before['phases']:
        if b['name'] in status_names:
            continue
        a = phase(after, b['name'])
        lines.append('| %s | %.1f → %.1f | %+.1f%% | %.1f → %.1f |' % (
            b['name'], b['server_cpu_ms'], a['server_cpu_ms'],
            pct(b['server_cpu_ms'], a['server_cpu_ms']),
            b['server_rss_peak_bytes']/2**20, a['server_rss_peak_bytes']/2**20))
    lines += ['', 'Held and released controls are checked for each shared worker '
        'and other-apparatus scope. Administrator loaded windows and sequence '
        'move/inverse match fresh HTTP, including pagination. A separate candidate '
        'two-OS-process check verifies PostgreSQL invalidation and exact authorized '
        'content across hold, release, move and inverse; it is not a distributed '
        'capacity test.', '', '## Focused verification', '',
        verification['backend'], '', verification['mobile'], '',
        'These are focused checks, not a full-suite pass claim.', '',
        'Commands and retained logs:', '',
        *['- `' + item + '`' for item in verification['commands_and_logs']], '',
        '## Limits and cleanup', '',
        'Loopback, one local PostgreSQL dataset and at most 32 sockets. No WAN, '
        'mobile rendering, battery, TLS or high-scale capacity claim. Both raw '
        'runs and outliers are retained. SQL logging, diagnostic tracing and '
        'period0 results are not substituted for default-configuration timing.', '',
        'Private artifacts: `' + str(root) + '`. Cleanup evidence is included in '
        'the JSON. Benchmark servers stopped: **%s**. PostgreSQL cleanup state: '
        '**%s**. Independent PostgreSQL sessions before cleanup: **%s**.' % (
            cleanup['benchmark_servers_stopped'], cleanup['postgres_state'],
            cleanup['independent_postgres_sessions_before_stop']), '']
    combined = {'primary': primary, 'reverse_repeat': repeat, 'sql_diagnostic': sql,
                'crossprocess': cross, 'cleanup': cleanup, 'verification': verification,
                'intermediate_cache_only_candidate': intermediate}
    out.mkdir(parents=True, exist_ok=True)
    stem = out / ('order_stream_cache_' + args.date)
    stem.with_suffix('.json').write_text(json.dumps(combined, indent=2) + '\n')
    stem.with_suffix('.md').write_text('\n'.join(lines))
    print(str(stem.with_suffix('.md')))


if __name__ == '__main__':
    main()
