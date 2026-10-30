//! Side effects latch independently of visible frames, including window creation.
use super::*;
impl<S: 'static> Runtime<S> {
    /// Latches a Tx's shell and clipboard requests for the shell, so `update`, a hidden poll
    /// and `on_close` reach the clipboard without a frame, and applies clipboard requests to a
    /// memory clipboard at once. A secret clear drops an earlier secret copy not yet written.
    pub(super) fn latch_requests(&mut self, tx: &mut Tx) {
        self.run_pick_reads(tx);
        self.shell_request = self.shell_request.max(tx.shell.take());
        self.minimize_request |= std::mem::take(&mut tx.minimize);
        self.latch_playground(tx);
        self.window_spawns.latch(&mut tx.window_spawns);
        if !tx.open_urls.is_empty() { self.open_urls(std::mem::take(&mut tx.open_urls)); }
        if std::mem::take(&mut tx.clear_secret) {
            if self.copy_request.as_ref().is_some_and(crate::ClipboardWrite::is_secret) { self.copy_request = None; }
            self.clear_secret_request = true;
            self.pill_copy = None;
        }
        if let Some(copy) = tx.copy.take() {
            self.pill_copy = (!copy.is_secret())
                .then(|| copy.content().html())
                .flatten()
                .filter(|h| h.contains("<span data-pill="))
                .map(|h| {
                    let frag = crate::TextEdit::from_html(h);
                    (
                        zeroize::Zeroizing::new(copy.content().text().to_owned()),
                        frag.pills().map(|(r, id)| (r, id.to_owned())).collect::<Vec<_>>(),
                    )
                })
                .filter(|(_, pills)| !pills.is_empty());
            self.copy_request = Some(copy);
        }
        if std::mem::take(&mut tx.paste) { self.paste_request = Some(self.paste_target()); }
        for read in tx.clipboard_reads.drain(..).filter_map(|read| read.take()) {
            self.next_clipboard_read += 1;
            self.clipboard_reads.push((self.next_clipboard_read, read));
            self.read_requests.push(self.next_clipboard_read);
        }
        self.popups.extend(tx.popups.drain(..).filter_map(|call| call.take()));
        for request in tx.file_dialogs.drain(..).filter_map(|call| call.take()) {
            self.request_file_dialog(request);
        }
        if let Some(srgb) = tx.srgb_blend.take() { self.set_srgb_blend(srgb); }
        if std::mem::take(&mut tx.blur) {
            if self.focused.is_some() {
                self.set_focus(None, self.keyboard_focus);
                self.dirty = true;
            }
            // An explicit blur is not the temporary pointer-down blur of a scrim.
            self.last_focused = None;
        }
        if let Some(title) = tx.title.take() { self.window_title.set(title); }
        if let Some(mut request) = tx.window_frame.take() {
            request.spring = (request.animate && !tx.instant && !self.reduced_motion).then(|| tx.motion.unwrap_or(motion::DEFAULT));
            self.window_frame = Some(self.window_frame.unwrap_or_default().merge(request));
        }
        crate::clipboard::flush_memory(self);
    }

}
