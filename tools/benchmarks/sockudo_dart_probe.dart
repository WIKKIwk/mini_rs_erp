// Real dart:io default compression interoperability probe, using the production
// mobile sequence reducer. Synthetic loopback data only; no authentication/DB.
// Run from mini_rs_erp with the sibling accord_mobile_v2 checkout present:
// dart tools/benchmarks/sockudo_dart_probe.dart --binary <release fixture>
import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../../../accord_mobile_v2/lib/src/core/realtime/sequence_delta.dart';

const apparatus = 'synthetic-apparatus';

String identifier(int value) =>
    '00000000-0000-4000-8000-${value.toRadixString(16).padLeft(12, '0')}';

Never fail(String reason) => throw StateError(reason);

// This SDK's WebSocket.extensions getter returns an empty string even after
// successful PMD negotiation. Observe the real HTTP response header while
// forwarding the unchanged native handshake and socket to dart:io.
class HandshakeClient implements HttpClient {
  final HttpClient inner = HttpClient();
  String extensions = '';

  @override
  Future<HttpClientRequest> openUrl(String method, Uri url) async =>
      HandshakeRequest(await inner.openUrl(method, url), this);

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

class HandshakeRequest implements HttpClientRequest {
  final HttpClientRequest inner;
  final HandshakeClient owner;
  HandshakeRequest(this.inner, this.owner);

  @override
  HttpHeaders get headers => inner.headers;

  @override
  Future<HttpClientResponse> close() async {
    final response = await inner.close();
    owner.extensions = response.headers.value('sec-websocket-extensions') ?? '';
    return response;
  }

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}

Map<String, dynamic> applyEvent(
  Map<String, dynamic> before,
  Map<String, dynamic> event,
) {
  if (event['type'] == 'snapshot') return event;
  if (event['type'] != 'state_delta' ||
      event['base_rev'] != before['rev'] ||
      event['epoch'] != before['epoch'] ||
      event['scope'] != before['scope']) {
    fail('Invalid stream revision, epoch or scope');
  }
  final after = jsonDecode(jsonEncode(before)) as Map<String, dynamic>;
  final patch = event['patch'] as Map<String, dynamic>;
  for (final field in patch.entries) {
    final changes = field.value as Map<String, dynamic>;
    final values = after[field.key] as Map<String, dynamic>;
    for (final key in changes['remove'] as List) {
      values.remove(key);
    }
    values.addAll(changes['upsert'] as Map<String, dynamic>);
    final edits = changes['edit'] as Map<String, dynamic>? ?? {};
    for (final edit in edits.entries) {
      final original = (before[field.key] as Map<String, dynamic>)[edit.key];
      if (original is! List) fail('Missing sequence baseline');
      values[edit.key] = applySequenceEdit(
        List<String>.from(original),
        edit.value,
      );
    }
  }
  after['rev'] = event['rev'];
  return after;
}

void verify(Map<String, dynamic> state, List<String> expected, int revision) {
  if (state['rev'] != revision) fail('Wrong revision');
  for (final field in ['sequences', 'visible_order_ids']) {
    final actual = List<String>.from((state[field] as Map)[apparatus] as List);
    if (actual.length != expected.length ||
        sequenceFingerprint(actual) != sequenceFingerprint(expected)) {
      fail('Wrong ordered sequence in $field');
    }
    for (var index = 0; index < actual.length; index++) {
      if (actual[index] != expected[index]) fail('Sequence content mismatch');
    }
  }
}

Future<Map<String, dynamic>> probe(
  int port,
  String profile,
  String protocol,
) async {
  const count = 1000;
  const updates = 16;
  final uri = Uri.http('127.0.0.1:$port', '/sockudo', {
    'count': '$count',
    'updates': '$updates',
    'scenario': 'reorder',
    'protocol': protocol,
    'profile': profile,
    'level': '3',
    'window': '15',
    'threshold': '128',
  });
  // Deliberately omit CompressionOptions: this is the app's default Dart offer.
  final handshake = HandshakeClient();
  final socket = await WebSocket.connect(
    uri.toString().replaceFirst('http:', 'ws:'),
    customClient: handshake,
  ).timeout(const Duration(seconds: 20));
  socket.pingInterval = const Duration(milliseconds: 100);
  if (!handshake.extensions.contains('permessage-deflate')) {
    fail('Default Dart compression was not negotiated');
  }
  final iterator = StreamIterator<dynamic>(socket);
  var decodedBytes = 0;
  Future<Map<String, dynamic>> next() async {
    if (!await iterator.moveNext().timeout(const Duration(seconds: 20))) {
      fail('Stream closed before expected update');
    }
    final frame = iterator.current;
    if (frame is! String) fail('Expected decoded JSON text frame');
    decodedBytes += utf8.encode(frame).length;
    return jsonDecode(frame) as Map<String, dynamic>;
  }

  var expected = List.generate(count, identifier);
  var state = await next();
  verify(state, expected, 0);
  for (var step = 0; step < updates; step++) {
    // Keep the IO stream polled while idle, so StreamIterator does not pause
    // transport reads and accidentally starve native Pong processing.
    final incoming = next();
    if (step == updates ~/ 2) {
      // Native Pings must get automatic Pongs even while the app sends nothing.
      await Future<void>.delayed(const Duration(milliseconds: 650));
      if (socket.readyState != WebSocket.open) {
        fail('Native Dart pingInterval closed an otherwise healthy stream');
      }
    }
    socket.add('{"type":"next"}');
    final source = (step * 7919) % count;
    final target = (step * 104729 + 1) % count;
    final moved = expected.removeAt(source);
    expected.insert(target, moved);
    state = applyEvent(state, await incoming);
    verify(state, expected, step + 1);
  }
  final result = <String, dynamic>{
    'profile': profile,
    'protocol': protocol,
    'queue_ids': count,
    'verified_frames': updates + 1,
    'final_revision': state['rev'],
    'decoded_json_bytes': decodedBytes,
    'extensions': handshake.extensions,
    'native_ping_interval_ms': socket.pingInterval!.inMilliseconds,
    'quiet_native_ping_phase_ms': 650,
    'sequence_fingerprint': sequenceFingerprint(expected),
    'production_mobile_reducer': true,
  };
  final ended = iterator.moveNext();
  await socket
      .close(WebSocketStatus.normalClosure, 'synthetic probe complete')
      .timeout(const Duration(seconds: 5));
  await ended.timeout(const Duration(seconds: 5));
  await iterator.cancel();
  handshake.inner.close(force: true);
  return result;
}

Future<void> main(List<String> arguments) async {
  if (arguments.length != 2 || arguments[0] != '--binary') {
    stderr.writeln(
      'Usage: dart sockudo_dart_probe.dart --binary <release fixture>',
    );
    exitCode = 64;
    return;
  }
  final process = await Process.start(File(arguments[1]).absolute.path, []);
  final stderrDrained = process.stderr.drain<void>();
  try {
    final line = await process.stdout
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .first
        .timeout(const Duration(seconds: 10));
    final fixture = jsonDecode(line) as Map<String, dynamic>;
    final cases = <Map<String, dynamic>>[];
    for (final profile in ['production', 'shared']) {
      for (final protocol in ['v1', 'v2']) {
        cases.add(await probe(fixture['port'] as int, profile, protocol));
      }
    }
    stdout.writeln(
      const JsonEncoder.withIndent('  ').convert({
        'dataset': 'synthetic',
        'runtime': Platform.version,
        'client': 'dart:io WebSocket.connect default CompressionOptions',
        'fixture_build': fixture,
        'cases': cases,
        'verified_frames': cases.fold<int>(
          0,
          (sum, item) => sum + (item['verified_frames'] as int),
        ),
        'result': 'passed',
      }),
    );
  } finally {
    process.kill();
    await process.exitCode.timeout(
      const Duration(seconds: 10),
      onTimeout: () {
        process.kill(ProcessSignal.sigkill);
        return -1;
      },
    );
    await stderrDrained;
  }
}
