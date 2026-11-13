import { useCallback, useRef, useSyncExternalStore } from "react";
import { api, listenBrowserChanged } from "./api";
import type { BrowserState } from "./types";
import { errorText } from "./ui";

/**
 * One copy of the built-in browser's state per window (the main window, each toolbar window).
 *
 * Rust numbers every state it hands out (`revision`): the `browser-state-changed` event carries
 * one, and so does every command result. A state older than the newest one seen is dropped, so a
 * slow read cannot undo a newer change. Reads that overlap are merged into one.
 * Address changes inside a page (history.pushState) send no event, so controls that show the
 * address ask for a slow re-read (`poll`) while they are on screen. Only one timer runs per window.
 */
export const BROWSER_POLL_MS = 1000;

export interface BrowserSnapshot {
  state: BrowserState | null;
  /** The last read failed. The previous state is kept. */
  error: string;
}

export interface BrowserSubscription {
  /** Re-read every BROWSER_POLL_MS while this subscriber is on screen. */
  poll?: boolean;
}

const EMPTY: BrowserSnapshot = { state: null, error: "" };

let snapshot: BrowserSnapshot = EMPTY;
let newest: number | null = null;
const listeners = new Set<() => void>();
let pollers = 0;
let reading = false;
let again = false;
let eventsGeneration = 0;
let stopEvents: (() => void) | null = null;
let timer: ReturnType<typeof setInterval> | null = null;

/** `next` is newer than every numbered state seen so far. Unnumbered states (the browser preview) always count. */
export function newerBrowserState(previous: number | null, next: Pick<BrowserState, "revision">): boolean {
  if (typeof next.revision !== "number" || previous === null) return true;
  return next.revision > previous;
}

function emit() {
  for (const listener of [...listeners]) listener();
}

export function getBrowserSnapshot(): BrowserSnapshot {
  return snapshot;
}

/** Takes a state from an event or a command result. Older states are ignored. */
export function applyBrowserState(next: BrowserState | null | undefined): void {
  if (!next || !newerBrowserState(newest, next)) return;
  if (typeof next.revision === "number") newest = next.revision;
  snapshot = { state: next, error: "" };
  emit();
}

/** Reads the state again. A read that is already running is followed by one more, not by several. */
export function refreshBrowserState(): void {
  if (reading) {
    again = true;
    return;
  }
  reading = true;
  void api
    .browserState()
    .then(applyBrowserState, (err: unknown) => {
      snapshot = { state: snapshot.state, error: errorText(err) };
      emit();
    })
    .finally(() => {
      reading = false;
      if (again) {
        again = false;
        refreshBrowserState();
      }
    });
}

function pageHidden(): boolean {
  return typeof document !== "undefined" && document.visibilityState === "hidden";
}

function syncPolling() {
  if (pollers > 0 && timer === null) {
    timer = setInterval(() => {
      if (!pageHidden()) refreshBrowserState();
    }, BROWSER_POLL_MS);
  } else if (pollers === 0 && timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

function startEvents() {
  const generation = ++eventsGeneration;
  void listenBrowserChanged((state) => {
    if (state) applyBrowserState(state);
    else refreshBrowserState();
  }).then(
    (unlisten) => {
      if (generation === eventsGeneration) stopEvents = unlisten;
      else unlisten();
    },
    () => undefined,
  );
}

function stopEventsNow() {
  eventsGeneration += 1;
  stopEvents?.();
  stopEvents = null;
}

/**
 * Follows the state. The first subscriber starts listening to the app and the last one stops it.
 * Nobody keeps an unwatched state: it may be stale by the next subscription, so it is dropped.
 */
export function subscribeBrowserState(listener: () => void, options: BrowserSubscription = {}): () => void {
  const poll = options.poll === true;
  listeners.add(listener);
  if (poll) pollers += 1;
  if (listeners.size === 1) startEvents();
  syncPolling();
  refreshBrowserState();
  let subscribed = true;
  return () => {
    if (!subscribed) return;
    subscribed = false;
    listeners.delete(listener);
    if (poll) pollers -= 1;
    syncPolling();
    if (listeners.size === 0) {
      stopEventsNow();
      snapshot = EMPTY;
    }
  };
}

/** The whole state and the last read error. */
export function useBrowserSnapshot(options: BrowserSubscription = {}): BrowserSnapshot {
  const poll = options.poll === true;
  const subscribe = useCallback((listener: () => void) => subscribeBrowserState(listener, { poll }), [poll]);
  return useSyncExternalStore(subscribe, getBrowserSnapshot, getBrowserSnapshot);
}

/**
 * Part of the state. The component draws again only when `isEqual` says the part changed,
 * not on every page title or loading change.
 */
export function useBrowserSelection<T>(
  select: (snapshot: BrowserSnapshot) => T,
  isEqual: (a: T, b: T) => boolean,
  options: BrowserSubscription = {},
): T {
  const poll = options.poll === true;
  const subscribe = useCallback((listener: () => void) => subscribeBrowserState(listener, { poll }), [poll]);
  const cache = useRef<{ source: BrowserSnapshot; value: T } | null>(null);
  const read = () => {
    const source = snapshot;
    const cached = cache.current;
    if (cached && cached.source === source) return cached.value;
    const value = select(source);
    const kept = cached && isEqual(cached.value, value) ? cached.value : value;
    cache.current = { source, value: kept };
    return kept;
  };
  return useSyncExternalStore(subscribe, read, read);
}
