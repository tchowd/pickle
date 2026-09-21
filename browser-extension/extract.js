/* Runs on demand in the extension's isolated world. No page scripts are evaluated. */
globalThis.pickleExtract = function(mode) {
  const bound = (text, bytes) => {
    let used = 0, result = '';
    for (const c of String(text || '')) { const n = new TextEncoder().encode(c).length; if (used+n > bytes) break; used += n; result += c; }
    return result;
  };
  const selection = bound(getSelection()?.toString(), 12000);
  const url = location.href, title = bound(document.title, 1000);
  if (!/^https?:$/.test(location.protocol)) throw new Error('Open a webpage first.');
  if (mode === 'article') {
    const clone = document.cloneNode(true);
    clone.querySelectorAll('input,textarea,[contenteditable],script,style,nav,footer').forEach(n => n.remove());
    const article = new Readability(clone, {maxElemsToParse: 20000}).parse();
    const text = bound(article?.textContent || document.querySelector('article,main')?.innerText || '', 12000);
    if (!text.trim()) throw new Error('No article found. Try selecting a passage.');
    return {selection, reference:{url,title,text,kind:'article',timestamp:null}};
  }
  const videos = [...document.querySelectorAll('video')].filter(v => v.getBoundingClientRect().width > 0);
  videos.sort((a,b) => Number(a.paused) - Number(b.paused) || b.clientWidth*b.clientHeight - a.clientWidth*a.clientHeight);
  const video = videos[0];
  if (!video) throw new Error('Open the video’s own page to use its captions.');
  const time = Number.isFinite(video.currentTime) ? video.currentTime : 0;
  const tracks = [...video.textTracks].filter(t => ['captions','subtitles'].includes(t.kind) && t.cues?.length);
  const track = tracks.find(t => t.mode === 'showing') || tracks[0];
  let cues = track ? [...track.cues].map(c => ({start:c.startTime,end:c.endTime,text:c.text})) : [];
  // YouTube's user-opened transcript; no undocumented caption endpoint or cookie export.
  if (!cues.length && /(^|\.)youtube\.com$/.test(location.hostname)) {
    cues = [...document.querySelectorAll('ytd-transcript-segment-renderer')].map(row => {
      const parts = (row.querySelector('.segment-timestamp')?.textContent || '').trim().split(':').map(Number);
      const start = parts.reduce((a,n) => a*60+n, 0);
      return {start,end:start+10,text:row.querySelector('.segment-text')?.textContent || ''};
    }).filter(c => Number.isFinite(c.start) && c.text.trim());
  }
  if (!cues.length) throw new Error('No readable captions. Open the transcript, or use Listen for 30 seconds in Pickle.');
  if (mode === 'moment') cues = cues.filter(c => c.end >= Math.max(0,time-90) && c.start <= time+10);
  if (!cues.length) throw new Error('No captions available near this moment.');
  const timestamp = n => `${Math.floor(n/60)}:${String(Math.floor(n%60)).padStart(2,'0')}`;
  const full = cues.map(c => `[${timestamp(c.start)}] ${c.text}`).join('\n');
  const truncated = new TextEncoder().encode(full).length > 11500;
  const text = bound(full,11500) + (truncated ? '\n[Transcript excerpt only; the remaining video is not included.]' : '\n[Available captions only; completeness has not been verified.]');
  return {selection, reference:{url,title,text,kind:mode === 'moment' ? 'video captions' : 'video transcript',timestamp:time}};
};
