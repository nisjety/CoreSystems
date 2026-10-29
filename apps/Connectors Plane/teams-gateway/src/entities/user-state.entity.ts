import { Entity, Column, PrimaryColumn, CreateDateColumn, UpdateDateColumn } from 'typeorm';

@Entity('user_state')
export class UserState {
  @PrimaryColumn({ name: 'user_id', type: 'text' })
  userId: string;

  @Column({ name: 'active_session_id', type: 'uuid', nullable: true })
  activeSessionId: string | null;

  @Column({ name: 'last_switcher_shown_at', type: 'timestamp', nullable: true })
  lastSwitcherShownAt: Date | null;

  @Column({ name: 'auto_name_enabled', type: 'boolean', default: true })
  autoNameEnabled: boolean;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;

  @UpdateDateColumn({ name: 'updated_at' })
  updatedAt: Date;
}
