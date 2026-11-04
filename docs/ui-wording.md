# Words for actions

One verb for one meaning, so a button says what it does before it is pressed. New panels follow these rules; Settings is the reference.

## Verbs

| Verb | Meaning | Examples |
|---|---|---|
| **Forget** | Ends a pairing or link with another device, host or person. Nothing on either side is deleted. | A paired device, a streaming host, a finished share |
| **Remove** | Takes something Caskade manages or lists out of Caskade. Its files stay. | An emulator, a ROM scanner, a storage location, a store account, a scanned folder, a PIN |
| **Delete** | Destroys something for good. | A script, a local account |
| **Uninstall** | Takes an add-on or a game off this device, files included. | A plugin, a theme, a game |
| **Discard** | Drops edits that were never saved. | The unsaved-changes question, a new server not saved yet, a scan review |
| **Disconnect** | Signs out of a store account and keeps its library. Only for store accounts. | Steam, GOG, Epic |

Check the verb against what the code does. Where Remove takes data with it, the confirmation says so rather than the verb changing: removing a RomM server deletes the ROMs downloaded from it, and its dialog says that.

**Revert** is not Discard: it returns an editor that stays open to the last saved version. **Clear** is for caches, which refill by themselves.

A newer version the user doesn't want is **Skip this version**, everywhere. Skip on its own is for a step in a flow, such as a wait before a game starts.

## Ellipses

A button or menu item ends with an ellipsis when it asks something before it acts: it opens a dialog or a file picker, or puts a question in its row. Update, Sync now and anything else that acts at once has none, and neither does a link to another page. The button that answers the question has none either: Remove… opens the dialog, Remove confirms it.

An editor that opens in place on the page isn't a dialog, so Add emulator and Add scanner have none.

An `aria-label` keeps the plain words (`Forget ${name}`), without the ellipsis.
