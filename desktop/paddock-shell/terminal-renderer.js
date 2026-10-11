const terminals = new Set();

/** Recalculate glyph positions without clearing the terminal buffer or the shared atlas. */
function refreshGlyphs(
    term,
) {
    term._core._renderService.clear();
    term.refresh(0, term.rows - 1);
}

/**
 * Keep every owner of a cleared atlas in sync, including terminals in hidden tabs.
 *
 * xterm 5.3 shares atlases but clearTextureAtlas only resets the caller's glyph model.
 * The active renderer's private atlas identifies the affected owners; using the active
 * renderer also excludes a WebGL addon that has already fallen back to DOM rendering.
 */
function synchronizeTextureAtlas(
    term,
) {
    const clearTextureAtlas = term.clearTextureAtlas;
    const clearSharedAtlas = () => {
        const atlas = term._core._renderService._renderer.value?._charAtlas;
        clearTextureAtlas.call(term);
        if (atlas) {
            for (const owner of terminals) {
                if (owner._core._renderService._renderer.value?._charAtlas === atlas) refreshGlyphs(owner);
            }
        }
    };
    terminals.add(term);
    term.clearTextureAtlas = clearSharedAtlas;
    return {
        dispose: () => {
            terminals.delete(term);
            if (term.clearTextureAtlas === clearSharedAtlas) term.clearTextureAtlas = clearTextureAtlas;
        },
    };
}

module.exports = { refreshGlyphs, synchronizeTextureAtlas };
