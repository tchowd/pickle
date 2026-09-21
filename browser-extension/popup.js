document.querySelectorAll('button').forEach(button => button.addEventListener('click', async () => {
  document.querySelectorAll('button').forEach(b => b.disabled = true);
  try {
    const result = await chrome.runtime.sendMessage({mode: button.dataset.mode});
    document.querySelector('#status').textContent = result.error || 'Ready in Pickle.';
  } catch { document.querySelector('#status').textContent = 'Could not connect. Check the Pickle browser setup.'; }
  finally { document.querySelectorAll('button').forEach(b => b.disabled = false); }
}));
