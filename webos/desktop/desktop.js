const launcher = document.querySelector('#launcher');
const workspace = document.querySelector('#workspace');
const shellForm = document.querySelector('#shell-form');
const shellCommand = document.querySelector('#shell-command');
const shellOutput = document.querySelector('#shell-output');

const apps = await fetch('/api/apps').then((response) => response.json());
for (const app of apps) {
  const button = document.createElement('button');
  button.textContent = app.title;
  button.addEventListener('click', () => openApp(app));
  launcher.append(button);
}

function openApp(app) {
  const card = document.createElement('section');
  card.className = 'window';
  card.innerHTML = `
    <header>
      <strong>${app.title}</strong>
      <button type="button">Close</button>
    </header>
    <iframe title="${app.title}" src="${app.entrypoint}" sandbox="${app.sandbox}"></iframe>
  `;
  card.querySelector('button').addEventListener('click', () => card.remove());
  workspace.prepend(card);
}

shellForm.addEventListener('submit', async (event) => {
  event.preventDefault();
  const command = shellCommand.value;
  const result = await fetch('/api/shell', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ command }),
  }).then((response) => response.json());
  shellOutput.textContent = [result.stdout, result.stderr].filter(Boolean).join('\n');
});
