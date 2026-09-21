async function send(mode) {
  if (!['article','moment','video'].includes(mode)) throw new Error('Unknown action.');
  const [tab] = await chrome.tabs.query({active:true,currentWindow:true});
  if (!tab?.id || !/^https?:\/\//.test(tab.url || '')) throw new Error('Open a webpage first.');
  if (tab.incognito) throw new Error('Private tabs are not supported.');
  await chrome.scripting.executeScript({target:{tabId:tab.id},files:['Readability.js','extract.js']});
  const [{result}] = await chrome.scripting.executeScript({target:{tabId:tab.id},func:mode => globalThis.pickleExtract(mode),args:[mode]});
  const current = await chrome.tabs.get(tab.id);
  if (current.url !== result.reference.url) throw new Error('The page changed. Try again.');
  return await chrome.runtime.sendNativeMessage('com.pickle.reader', {...result, id:crypto.randomUUID(), capturedAt:Date.now()/1000});
}
chrome.runtime.onMessage.addListener((message,sender,reply) => {
  if (sender.id !== chrome.runtime.id) return;
  send(message.mode).then(reply).catch(error => reply({error:error.message}));
  return true;
});
chrome.commands.onCommand.addListener(command => { if(command === 'send-page') send('article').catch(() => chrome.action.setBadgeText({text:'!'})); });
