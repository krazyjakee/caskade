/** The splash `index.html` shows while the app starts (`#boot`): it fades
 * out once the first screen is ready. */
export function hideBootSplash(doc: Document = document) {
  const splash = doc.getElementById("boot");
  if (!splash || splash.dataset.state === "done") return;
  splash.dataset.state = "done";
  const remove = () => splash.remove();
  splash.addEventListener("transitionend", remove, { once: true });
  // No transition runs with reduced motion, or in the background.
  setTimeout(remove, 800);
}
