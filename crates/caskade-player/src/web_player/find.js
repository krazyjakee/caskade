(() => {
  // HTML wrappers sometimes fetch the player, then frame a Blob made from
  // its text. Keep the original HTTP address: a blob URL dies with the page.
  // Observe only bodies the page already reads, bounded to four small HTML
  // documents. Nothing reads or copies the game's large Unity data files.
  const pages = new Map();
  const bodies = new WeakMap();
  const blobSources = new Map();
  const limit = 2 * 1024 * 1024;
  const bounded = (map, key, value, count) => {
    map.set(key, value);
    while (map.size > count) map.delete(map.keys().next().value);
  };
  const htmlSource = (response) => {
    try {
      const url = new URL(response.url);
      return /^https?:$/.test(url.protocol) &&
        /^text\/(?:html|plain)(?:;|$)/i.test(response.headers.get("content-type") || "")
        ? url.href : null;
    } catch { return null; }
  };
  const readText = Response.prototype.text;
  Response.prototype.text = function () {
    return readText.call(this).then((text) => {
      const source = htmlSource(this);
      if (source && text.length <= limit) {
        const previous = pages.get(text);
        bounded(pages, text, previous === undefined || previous === source ? source : null, 4);
      }
      return text;
    });
  };
  const readBlob = Response.prototype.blob;
  Response.prototype.blob = function () {
    return readBlob.call(this).then((blob) => {
      const source = htmlSource(this);
      if (source && blob.size <= limit) bodies.set(blob, source);
      return blob;
    });
  };
  const remember = (href, source) => {
    if (!source) return;
    bounded(blobSources, href, source, 8);
    if (window !== window.top) {
      window.top.postMessage({ __caskadeBlob: { href, source } }, "*");
    }
  };
  const createBlob = URL.createObjectURL;
  URL.createObjectURL = function (blob) {
    const href = createBlob.call(this, blob);
    if (blob.size <= limit && /^text\/html(?:;|$)/i.test(blob.type)) {
      if (bodies.has(blob)) remember(href, bodies.get(blob));
      else blob.text().then((text) => remember(href, pages.get(text))).catch(() => {});
    }
    return href;
  };
  if (window === window.top) {
    const shown = new WeakMap();
    addEventListener("message", (event) => {
      const frame = event.data && event.data.__caskadeFrame;
      if (frame && typeof frame.href === "string" && event.source) shown.set(event.source, frame);
      const blob = event.data && event.data.__caskadeBlob;
      if (blob && typeof blob.href === "string" && typeof blob.source === "string" &&
          blob.href.startsWith("blob:") && /^https?:\/\//.test(blob.source)) {
        bounded(blobSources, blob.href, blob.source, 8);
      }
    });
    const filling = (win, width, height, depth) => {
      let best = null;
      const own = shown.get(win);
      if (own && own.width * own.height >= 0.8 * width * height) best = { href: own.href, depth };
      let count = 0;
      try {
        count = depth < 4 ? win.length : 0;
      } catch {}
      for (let i = 0; i < count; i++) {
        let inner = null;
        try {
          inner = win[i];
        } catch {}
        const found = inner && filling(inner, width, height, depth + 1);
        if (found && (!best || found.depth > best.depth)) best = found;
      }
      return best;
    };
    const kinds = "iframe, embed, object, ruffle-embed, ruffle-object, canvas, video";
    Object.defineProperty(window, "__caskadeFind", {
      value: () => {
        const found = [];
        for (const element of document.querySelectorAll(kinds)) {
          const box = element.getBoundingClientRect();
          if (box.width < 320 || box.height < 240) continue;
          const style = getComputedStyle(element);
          if (style.visibility === "hidden" || Number(style.opacity) === 0) continue;
          const kind = element.localName;
          let src = null;
          if (kind === "iframe") {
            const inner = element.contentWindow && filling(element.contentWindow, box.width, box.height, 0);
            src = inner ? inner.href : element.src;
          } else if (kind.endsWith("embed")) src = element.getAttribute("src");
          else if (kind.endsWith("object")) src = element.getAttribute("data");
          try {
            src = src ? new URL(src, document.baseURI).href : null;
          } catch {
            src = null;
          }
          const original = blobSources.get(src);
          found.push({ kind, src: original || src, fromBlob: !!original,
            width: Math.round(box.width), height: Math.round(box.height) });
        }
        return found.sort((a, b) => b.width * b.height - a.width * a.height).slice(0, 8);
      },
    });
  } else {
    const tell = () => {
      try {
        window.top.postMessage(
          { __caskadeFrame: { href: location.href, width: innerWidth, height: innerHeight } },
          "*",
        );
      } catch {}
    };
    tell();
    addEventListener("load", tell);
    addEventListener("resize", tell);
  }
})();
