import type { ReactNode } from 'react';

interface PageHeaderProps {
  section: string;
  title: string;
  /** Mono readout beside the title, e.g. "42 tracks · 3 h 12 min left". */
  meta?: ReactNode;
  actions?: ReactNode;
}

/** Caption, title and actions at the top of a view (Albums, Queue… boards). */
export function PageHeader({ section, title, meta, actions }: PageHeaderProps) {
  return (
    <div className="page-header">
      <div className="flex min-w-0 flex-col gap-1">
        <div className="cap">{section}</div>
        <div className="flex min-w-0 items-baseline gap-3">
          <h1 className="page-title">{title}</h1>
          {meta && <span className="mono t3 el text-[11.5px]">{meta}</span>}
        </div>
      </div>
      {actions && <div className="flex flex-none items-center gap-2">{actions}</div>}
    </div>
  );
}
