import type { ReactNode } from 'react';

import { Icon } from '@/components/icons/Icon';
import type { IconName } from '@/components/icons/paths';

interface EmptyCardProps {
  icon: IconName;
  title: string;
  children?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

/** A compact empty-state card (Empty states · 03). */
export function EmptyCard({ icon, title, children, actions, className }: EmptyCardProps) {
  return (
    <div className={['empty-card', className].filter(Boolean).join(' ')}>
      <Icon name={icon} size={26} className="t3" />
      <span className="empty-card-title">{title}</span>
      {children && <div className="empty-card-body">{children}</div>}
      {actions && <div className="empty-card-actions">{actions}</div>}
    </div>
  );
}
