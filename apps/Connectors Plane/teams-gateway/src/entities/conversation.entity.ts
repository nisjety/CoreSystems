import { Entity, PrimaryGeneratedColumn, Column, CreateDateColumn, UpdateDateColumn, ManyToOne, JoinColumn, Index } from 'typeorm';
import { User } from './user.entity';

@Entity('conversations')
@Index(['teams_conversation_id'], { unique: true })
export class Conversation {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ type: 'varchar', length: 255, unique: true })
  teams_conversation_id: string;

  @Column({ type: 'varchar', length: 50 })
  conversation_type: string; // 'personal', 'channel', 'groupchat'

  @Column({ type: 'varchar', length: 255, nullable: true })
  channel_id: string;

  @Column({ type: 'varchar', length: 255, nullable: true })
  team_id: string;

  @Column({ type: 'varchar', length: 255, nullable: true })
  service_url: string;

  @ManyToOne(() => User, { nullable: true })
  @JoinColumn({ name: 'user_id' })
  user: User;

  @Column({ type: 'uuid', nullable: true })
  user_id: string;

  @Column({ type: 'json', nullable: true })
  context_data: Record<string, any>;

  @Column({ type: 'boolean', default: true })
  is_active: boolean;

  @CreateDateColumn()
  created_at: Date;

  @UpdateDateColumn()
  updated_at: Date;
}
