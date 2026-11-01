fetch('data/level1.json');
const x = new XMLHttpRequest(); x.open('GET', 'data/level2.json'); x.send();
new Worker('worker.js');
WebAssembly.instantiateStreaming(fetch('mod.wasm')).catch(() => {});
import('./lazy.js');
new Image().src = 'sprite.png';
setTimeout(() => fetch('data/later.json'), 3000);
