"""Protocol fixture. Never reads a real Codex home or calls a model."""
import json
import os
import sys
import time
from pathlib import Path

sys.stdin.reconfigure(encoding='utf-8')
sys.stdout.reconfigure(encoding='utf-8')

mode = os.environ.get('TEST_MODE', 'normal')
name = 'Existing local title' if mode == 'preserve' else None
cwd = os.environ.get('TEST_CODEX_HOME')
archived = mode in ('archived', 'archived_empty', 'unarchive')
thread_id = os.environ['TEST_THREAD_ID']

for line in sys.stdin:
    request = json.loads(line)
    with Path(os.environ['TEST_RPC_LOG']).open('a', encoding='utf-8') as log:
        log.write(json.dumps(request) + '\n')
    method = request['method']
    if 'id' not in request:
        continue
    result = {}
    error = None
    if method == 'initialize':
        result = {'codexHome': os.environ['TEST_CODEX_HOME']}
    elif method == 'thread/resume':
        if mode == 'timeout':
            time.sleep(20)
            continue
        if archived:
            error = {'code': -32000, 'message': 'Thread is archived'}
        else:
            cwd = request['params'].get('cwd', cwd)
            result = {'thread': {'id': 'ffffffff-ffff-ffff-ffff-ffffffffffff' if mode == 'wrong_id' else thread_id}}
    elif method == 'thread/unarchive':
        archived = False
    elif method == 'thread/name/set':
        if archived:
            error = {'code': -32600, 'message': 'no rollout found for archived thread'}
        if mode != 'bad_name':
            name = request['params']['name']
    elif method == 'thread/read':
        result = {'thread': {'id': thread_id, 'name': name, 'cwd': cwd,
                             'status': {'type': 'active' if mode == 'active' else 'notLoaded'}}}
    elif method == 'thread/turns/list':
        result = {'data': [] if mode in ('empty', 'archived_empty') else [{'id': 'saved-turn'}], 'nextCursor': 'older' if mode == 'normal' else None}
    else:
        error = {'code': -32601, 'message': 'Unexpected method'}
    # Notifications and non-JSON diagnostics must not be mistaken for replies.
    print('fixture diagnostic', flush=True)
    print(json.dumps({'method': 'fixture/event', 'params': {}}), flush=True)
    response = {'id': request['id']}
    response['error' if error else 'result'] = error or result
    print(json.dumps(response, ensure_ascii=True), flush=True)
