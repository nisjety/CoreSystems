import { Entity, Column, PrimaryGeneratedColumn, CreateDateColumn, UpdateDateColumn, OneToMany, Index } from 'typeorm';
import { SessionMessage } from './session-message.entity';

export enum SessionStatus {
  ACTIVE = 'active',
  ARCHIVED = 'archived'
}

@Entity('user_sessions')
@Index(['userId', 'status'])
@Index(['userId', 'updatedAt'])
export class UserSession {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ name: 'user_id', type: 'text' })
  @Index()
  userId: string;

  @Column({ type: 'text', default: 'Ny samtale' })
  title: string;

  @Column({ 
    type: 'enum', 
    enum: SessionStatus, 
    default: SessionStatus.ACTIVE 
  })
  status: SessionStatus;

  @Column({ name: 'is_pinned', type: 'boolean', default: false })
  isPinned: boolean;

  @Column({ name: 'message_count', type: 'integer', default: 0 })
  messageCount: number;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;

  @UpdateDateColumn({ name: 'updated_at' })
  updatedAt: Date;

  @OneToMany(() => SessionMessage, message => message.session, { cascade: true })
  messages: SessionMessage[];
}
