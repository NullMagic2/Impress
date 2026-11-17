# Links and the clipboard

Hold **Ctrl** and click a hyperlink to follow its URL using your system's default
browser. A normal click places the text cursor so you can edit the link's label.
Hovering shows the target and the Ctrl+click hint. Links to document bookmarks
move to the bookmarked text inside Impress.

Right-click the page for **Cut**, **Copy** and **Paste**. These commands remain
available alongside spelling corrections when right-clicking an underlined word.
An existing text or table-cell selection is preserved when clicking inside it.
Cut and Copy require a selection; Paste uses the system clipboard.

**Ctrl+X**, **Ctrl+C** and **Ctrl+V** use the same operations as the menu. These
shortcuts work with Greek and other non-Latin keyboard layouts. **Ctrl+Shift+V**
pastes text without formatting.

On Linux, Impress exchanges Unicode text, HTML, RTF and PNG through GTK's system
clipboard, on X11 and Wayland. Transfers between Impress documents also carry the
semantic fragment, preserving document formatting and resources. Clipboard reads
from another application run asynchronously, and clipboard managers can retain
the published formats when Impress closes. On Windows, the clipboard includes
Unicode text, CF_HTML, RTF, images and Impress's semantic format.
