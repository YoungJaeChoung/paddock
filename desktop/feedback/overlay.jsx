import React, { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { Agentation } from 'agentation';

const pending = new Map();
const unsavedKey = 'herdr-feedback-unsaved';
let n_requests = 0;

window.__herdrFeedbackReply = (id, reply) => {
    const resolve = pending.get(id);
    if (resolve) {
        pending.delete(id);
        resolve(reply);
    }
};

function request(
    operation,
    payload,
) {
    const id = ++n_requests;
    const result = new Promise((resolve) => {
        pending.set(id, resolve);
        window.herdrFeedbackBridge(JSON.stringify({ id, operation, payload }));
    });
    return result;
}

function readText(
    element,
) {
    const value = (element?.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 80);
    return value;
}

function readBox(
    element,
) {
    const box = element.getBoundingClientRect();
    return { x: box.x, y: box.y, width: box.width, height: box.height };
}

function findElement(
    box,
) {
    const hits = document.elementsFromPoint(box.x + box.width / 2, box.y + box.height / 2);
    const element = hits.find((item) => !item.closest('[data-agentation-root], [data-agentation-toolbar]')) || null;
    return element;
}

function readTarget(
    box,
) {
    const element = findElement(box);
    const targetBox = element && box.width < 5 && box.height < 5 ? readBox(element) : box;
    const penPath = [];
    let named = element?.closest('[data-pen]') || null;
    while (named) {
        penPath.push(named.getAttribute('data-pen'));
        named = named.parentElement?.closest('[data-pen]') || null;
    }
    const unnamed = element ? {
        tag: element.tagName.toLowerCase(),
        classes: typeof element.className === 'string' ? element.className : '',
        text: element.getAttribute('aria-label') || readText(element),
    } : null;
    return { penPath, unnamed, box: targetBox };
}

function readTargets(
    annotation,
) {
    const rawBoxes = annotation.elementBoundingBoxes?.length
        ? annotation.elementBoundingBoxes : [annotation.boundingBox || {
            x: annotation.x,
            y: annotation.y,
            width: 0,
            height: 0,
        }];
    const targets = rawBoxes.map((raw) => {
        const box = { ...raw, y: annotation.isFixed ? raw.y : raw.y - window.scrollY };
        return readTarget(box);
    });
    return targets;
}

function composeNote(
    annotation,
) {
    const targets = readTargets(annotation);
    const first = targets[0] || { penPath: [], unnamed: null };
    const note = {
        id: annotation.id,
        takenAt: new Date(annotation.timestamp).toISOString(),
        comment: annotation.comment,
        targets,
        penPath: first.penPath,
        unnamed: first.unnamed,
        screen: [...document.querySelectorAll('[data-pen], [aria-label]')]
            .filter((element) => {
                const box = element.getBoundingClientRect();
                return box.width > 0 && box.height > 0 && !element.closest('[data-agentation-root], [data-herdr-feedback]');
            })
            .slice(0, 200)
            .map((element) => ({
                pen: element.getAttribute('data-pen') || element.getAttribute('aria-label') || '',
                depth: 0,
                text: readText(element),
            })),
        title: document.title,
        viewport: { width: window.innerWidth, height: window.innerHeight },
        url: window.location.href,
        story: null,
        raw: annotation,
        capture: 'current-renderer',
    };
    return note;
}

function pruneProcessed(
    ids,
) {
    const remaining = new Set(ids);
    const unsaved = readUnsaved();
    for (const id of unsaved) remaining.add(id);
    for (let index = window.localStorage.length - 1; index >= 0; index -= 1) {
        const key = window.localStorage.key(index);
        if (!key?.startsWith('feedback-annotations-')) continue;
        let stored;
        try {
            stored = JSON.parse(window.localStorage.getItem(key));
        } catch {
            continue;
        }
        if (!Array.isArray(stored)) continue;
        const kept = stored.filter((item) => remaining.has(item.id));
        if (kept.length === 0) window.localStorage.removeItem(key);
        else if (kept.length !== stored.length) window.localStorage.setItem(key, JSON.stringify(kept));
    }
}

function readUnsaved() {
    let ids = [];
    try {
        const stored = JSON.parse(window.localStorage.getItem(unsavedKey) || '[]');
        if (Array.isArray(stored)) ids = stored.filter((id) => typeof id === 'string');
    } catch {
        // 다른 피드백 자료는 그대로 두고 이 목록만 무시한다.
    }
    return ids;
}

function FeedbackOverlay() {
    const [ready, setReady] = useState(false);
    const [error, setError] = useState('');
    const [portalHost, setPortalHost] = useState(null);

    useEffect(() => {
        let active = true;
        request('list').then((reply) => {
            if (reply.ok) pruneProcessed(reply.ids);
            else setError(reply.message);
            if (active) setReady(true);
        });
        return () => { active = false; };
    }, []);

    useEffect(() => {
        function updatePortalHost() {
            const dialog = [...document.querySelectorAll('dialog')].find((item) => item.matches(':modal'));
            const host = dialog || null;
            window.__herdrFeedbackPortal = host;
            setPortalHost(host);
        }
        const observer = new MutationObserver(updatePortalHost);
        observer.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['open'] });
        updatePortalHost();
        return () => observer.disconnect();
    }, []);

    async function add(
        annotation,
    ) {
        const reply = await request('save', composeNote(annotation));
        const unsaved = new Set(readUnsaved());
        if (!reply.ok) {
            unsaved.add(annotation.id);
            setError(`메모를 저장하지 못했습니다: ${reply.message}`);
        } else {
            unsaved.delete(annotation.id);
            setError('');
        }
        window.localStorage.setItem(unsavedKey, JSON.stringify([...unsaved]));
    }

    return <>
        {ready && <Agentation key={portalHost ? 'dialog' : 'body'} onAnnotationAdd={(annotation) => { void add(annotation); }} copyToClipboard={false} />}
        {error && <div data-herdr-feedback-error style={{ position: 'fixed', right: 20, bottom: 72, zIndex: 2147483647, padding: 12, background: '#7f1d1d', color: '#fff', borderRadius: 8 }}>{error}</div>}
    </>;
}

function mountFeedback() {
    if (!document.querySelector('[data-herdr-feedback]')) {
        const host = document.createElement('div');
        host.setAttribute('data-herdr-feedback', '');
        document.body.append(host);
        createRoot(host).render(<FeedbackOverlay />);
    }
}

if (document.body) mountFeedback();
else document.addEventListener('DOMContentLoaded', mountFeedback, { once: true });
