import { Entity, Column, PrimaryGeneratedColumn, CreateDateColumn, ManyToOne, JoinColumn, Index } from 'typeorm';
import { UserSession } from './user-session.entity';

export enum MessageRole {
  USER = 'user',
  ASSISTANT = 'assistant',
  TOOL = 'tool'
}

@Entity('session_messages')
@Index(['sessionId', 'createdAt'])
export class SessionMessage {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ name: 'session_id', type: 'uuid' })
  @Index()
  sessionId: string;

  @Column({ 
    type: 'enum', 
    enum: MessageRole 
  })
  role: MessageRole;

  @Column({ type: 'text' })
  content: string;

  @Column({ type: 'jsonb', nullable: true })
  metadata: Record<string, any>;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;

  @ManyToOne(() => UserSession, session => session.messages, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'session_id' })
  session: UserSession;
}
