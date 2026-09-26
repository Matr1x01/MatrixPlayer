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
      kind: 'custom';
      title: string;
      render: (close: () => void) => ReactNode;
      width?: number;
      resolve: () => void;
    };

const useDialog = create<{ dialog: DialogSpec | null; closing: boolean }>()(() => ({ dialog: null, closing: false }));

function show(spec: DialogSpec) {
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
    window.setTimeout(() => useDialog.setState({ dialog: null, closing: false }), EXIT_MS);
    (dialog.resolve as (v: unknown) => void)(value);
  };
  const cancel = () => finish(dialog.kind === 'prompt' ? null : dialog.kind === 'confirm' ? false : undefined);
  const submit = () => {
    if (dialog.kind === 'prompt') {
      if (text.trim()) finish(text.trim());
    } else if (dialog.kind === 'confirm') finish(true);
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
            <div className="dlg-actions">
              <button type="button" className="btn" onClick={cancel}>
                Cancel
              </button>
              <button
                type="submit"
                className={dialog.kind === 'confirm' && dialog.danger ? 'btn danger' : 'btn pri'}
                disabled={dialog.kind === 'prompt' && !text.trim()}
                autoFocus={dialog.kind === 'confirm'}
              >
                {dialog.confirm}
              </button>
            </div>
          </form>
        )}
      </section>
    </div>
  );
}
