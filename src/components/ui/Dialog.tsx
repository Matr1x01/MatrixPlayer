import { useEffect, useRef, useState, type ReactNode } from 'react';
import { create } from 'zustand';

import { Icon } from '@/components/icons/Icon';

type DialogSpec =
  | {
      kind: 'prompt';
      title: string;
      label?: string;
      initial?: string;
      placeholder?: string;
      confirm: string;
      resolve: (value: string | null) => void;
    }
  | {
      kind: 'confirm';
      title: string;
      body: ReactNode;
      confirm: string;
      danger?: boolean;
      resolve: (ok: boolean) => void;
    }
  | {
      kind: 'choice';
      title: string;
      body: ReactNode;
      /** Buttons left to right; the last is the default. */
      choices: { label: string; value: string }[];
      resolve: (value: string | null) => void;
    }
  | {
      kind: 'custom';
      title: string;
      render: (close: () => void) => ReactNode;
      width?: number;
      resolve: () => void;
    };

const useDialog = create<{ dialog: DialogSpec | null; closing: boolean }>()(() => ({ dialog: null, closing: false }));

/** Where focus was when the dialog opened, to go back to when it closes. */
let returnFocus: HTMLElement | null = null;

function show(spec: DialogSpec) {
  const active = document.activeElement;
  returnFocus = active instanceof HTMLElement && active !== document.body ? active : null;
  useDialog.setState({ dialog: spec, closing: false });
}

/** Asks for a line of text; resolves to null when cancelled. */
export function askText(options: { title: string; label?: string; initial?: string; placeholder?: string; confirm?: string }) {
  return new Promise<string | null>((resolve) =>
    show({ kind: 'prompt', confirm: 'Save', ...options, resolve }),
  );
}

export function askConfirm(options: { title: string; body: ReactNode; confirm: string; danger?: boolean }) {
  return new Promise<boolean>((resolve) => show({ kind: 'confirm', ...options, resolve }));
}

/** Offers several answers; resolves to the chosen value, or null when cancelled. */
export function askChoice(options: { title: string; body: ReactNode; choices: { label: string; value: string }[] }) {
  return new Promise<string | null>((resolve) => show({ kind: 'choice', ...options, resolve }));
}

export function showDialog(title: string, render: (close: () => void) => ReactNode, width?: number) {
  return new Promise<void>((resolve) => show({ kind: 'custom', title, render, width, resolve }));
}

const EXIT_MS = 140;

/** The modal layer: one dialog at a time, over a dimmed, blurred app. */
export function DialogHost() {
  const { dialog, closing } = useDialog();
  const [text, setText] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (dialog?.kind === 'prompt') {
      setText(dialog.initial ?? '');
      requestAnimationFrame(() => inputRef.current?.select());
    }
  }, [dialog]);

  if (!dialog) return null;

  const finish = (value: unknown) => {
    useDialog.setState({ closing: true });
    window.setTimeout(() => {
      const target = returnFocus;
      returnFocus = null;
      useDialog.setState({ dialog: null, closing: false });
      // Back to where the user was once the dialog is gone, unless the
      // action moved focus somewhere on purpose.
      requestAnimationFrame(() => {
        const active = document.activeElement;
        const lost = !active || active === document.body || active.closest('.dlg-scrim') != null;
        if (target?.isConnected && lost) target.focus({ preventScroll: true });
      });
    }, EXIT_MS);
    (dialog.resolve as (v: unknown) => void)(value);
  };
  const cancel = () =>
    finish(dialog.kind === 'prompt' || dialog.kind === 'choice' ? null : dialog.kind === 'confirm' ? false : undefined);
  const submit = () => {
    if (dialog.kind === 'prompt') {
      if (text.trim()) finish(text.trim());
    } else if (dialog.kind === 'confirm') finish(true);
    else if (dialog.kind === 'choice') finish(dialog.choices.at(-1)?.value ?? null);
    else finish(undefined);
  };

  return (
    <div
      className={closing ? 'dlg-scrim closing' : 'dlg-scrim'}
      onPointerDown={(e) => e.target === e.currentTarget && cancel()}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation();
          cancel();
        }
      }}
    >
      <section
        className="dlg"
        role="dialog"
        aria-modal="true"
        aria-label={dialog.title}
        style={dialog.kind === 'custom' && dialog.width ? { width: dialog.width } : undefined}
      >
        <header className="dlg-head">
          <span className="font-bold text-[15px]">{dialog.title}</span>
          <button type="button" className="ib" style={{ width: 28, height: 28 }} aria-label="Close" onClick={cancel}>
            <Icon name="x" size={15} />
          </button>
        </header>
        {dialog.kind === 'custom' ? (
          <div className="dlg-body">{dialog.render(() => finish(undefined))}</div>
        ) : (
          <form
            className="dlg-body"
            onSubmit={(e) => {
              e.preventDefault();
              submit();
            }}
          >
            {dialog.kind === 'prompt' ? (
              <label className="flex flex-col gap-1.5">
                {dialog.label && <span className="t2 text-[12px]">{dialog.label}</span>}
                <input
                  ref={inputRef}
                  className="fld-input"
                  value={text}
                  placeholder={dialog.placeholder}
                  maxLength={200}
                  onChange={(e) => setText(e.target.value)}
                  autoFocus
                />
              </label>
            ) : (
              <div className="t2 text-[13px] [text-wrap:pretty]">{dialog.body}</div>
            )}
            {dialog.kind === 'choice' ? (
              <div className="dlg-actions">
                <button type="button" className="btn" onClick={cancel}>
                  Cancel
                </button>
                {dialog.choices.map((c, i) => {
                  const last = i === dialog.choices.length - 1;
                  return (
                    <button key={c.value} type="button" className={last ? 'btn pri' : 'btn'} autoFocus={last} onClick={() => finish(c.value)}>
                      {c.label}
                    </button>
                  );
                })}
              </div>
            ) : (
              <div className="dlg-actions">
                {/* A destructive confirmation starts on Cancel, so Enter
                    cannot remove or delete anything by reflex. */}
                <button type="button" className="btn" onClick={cancel} autoFocus={dialog.kind === 'confirm' && dialog.danger}>
                  Cancel
                </button>
                <button
                  type="submit"
                  className={dialog.kind === 'confirm' && dialog.danger ? 'btn danger' : 'btn pri'}
                  disabled={dialog.kind === 'prompt' && !text.trim()}
                  autoFocus={dialog.kind === 'confirm' && !dialog.danger}
                >
                  {dialog.confirm}
                </button>
              </div>
            )}
          </form>
        )}
      </section>
    </div>
  );
}
