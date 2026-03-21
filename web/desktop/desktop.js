const launcher = document.querySelector('#launcher');
const workspace = document.querySelector('#workspace');
const clock = document.querySelector('#status-clock');

setInterval(() => {
  clock.textContent = new Date().toISOString().replace('T', ' ').slice(0, 19) + ' UTC';
}, 1000);

async function fetchApps() {
  const response = await fetch('/api/apps');
  const payload = await response.json();
  return payload.apps;
}

function createWindow(app) {
  const shell = document.createElement('section');
  shell.className = 'window';
  shell.innerHTML = `
    <header>
      <strong>${app.name}</strong>
      <button type="button">Close</button>
    </header>
  `;
  const frame = document.createElement('iframe');
  frame.src = app.entry;
  frame.sandbox = 'allow-scripts allow-same-origin';
  shell.appendChild(frame);
  shell.querySelector('button').addEventListener('click', () => shell.remove());
  workspace.appendChild(shell);
}

function addLauncherItem(app) {
  const button = document.createElement('button');
  button.className = 'launcher-item';
  button.type = 'button';
  button.innerHTML = `<strong>${app.name}</strong><br><small>${app.description}</small>`;
  button.addEventListener('click', () => createWindow(app));
  launcher.appendChild(button);
}

fetchApps().then(apps => apps.forEach(addLauncherItem));
