async function postJson(path, body) {
  const response = await fetch(path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body)
  });
  return response.json();
}

export function createAppApi(appId) {
  return {
    list(scope = 'app', dir = '.') {
      return postJson('/api/fs/list', { scope, appId, path: dir });
    },
    read(path) {
      return postJson('/api/fs/read', { scope: 'app', appId, path });
    },
    write(path, content) {
      return postJson('/api/fs/write', { scope: 'app', appId, path, content });
    },
    remove(path) {
      return postJson('/api/fs/delete', { scope: 'app', appId, path });
    },
    mkdir(path) {
      return postJson('/api/fs/mkdir', { scope: 'app', appId, path });
    },
    sharedList(dir = '.') {
      return postJson('/api/fs/list', { scope: 'shared', path: dir });
    },
    executeWasm(program, args = []) {
      return postJson('/api/wasm/execute', { program, args, appId });
    }
  };
}
