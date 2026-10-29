import { MigrationInterface, QueryRunner } from 'typeorm';

// Generated manually based on entity definitions.
// Safe to run on an empty database. If tables already exist, you may need to adjust or start from a fresh DB.
export class InitialSchema1726040000000 implements MigrationInterface {
  name = 'InitialSchema1726040000000';

  public async up(queryRunner: QueryRunner): Promise<void> {
    // ENUM types
    await queryRunner.query(`DO $$ BEGIN
      IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'sessionstatus') THEN
        CREATE TYPE sessionstatus AS ENUM ('active','archived');
      END IF;
      IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'messagerole') THEN
        CREATE TYPE messagerole AS ENUM ('user','assistant','tool');
      END IF;
    END $$;`);

    // users
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS users (
      id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      teams_user_id varchar(255) UNIQUE NOT NULL,
      display_name varchar(255),
      email varchar(255),
      tenant_id varchar(100),
      encrypted_data_vault text,
      is_active boolean DEFAULT true NOT NULL,
      last_activity_at timestamp,
      preferences json,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMP NOT NULL DEFAULT NOW()
    );`);
    await queryRunner.query(`CREATE UNIQUE INDEX IF NOT EXISTS IDX_users_teams_user_id ON users(teams_user_id);`);

    // conversations
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS conversations (
      id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      teams_conversation_id varchar(255) UNIQUE NOT NULL,
      conversation_type varchar(50) NOT NULL,
      channel_id varchar(255),
      team_id varchar(255),
      service_url varchar(255),
      user_id uuid,
      context_data json,
      is_active boolean DEFAULT true NOT NULL,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
      CONSTRAINT FK_conversations_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
    );`);
    await queryRunner.query(`CREATE UNIQUE INDEX IF NOT EXISTS IDX_conversations_teams_conversation_id ON conversations(teams_conversation_id);`);

    // messages
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS messages (
      id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      teams_activity_id varchar(255) UNIQUE NOT NULL,
      conversation_id uuid NOT NULL,
      user_id uuid,
      message_type varchar(50) NOT NULL,
      content text,
      encrypted_content text,
      attachments json,
      metadata json,
      parent_message_id varchar(100),
      is_edited boolean DEFAULT false NOT NULL,
      is_deleted boolean DEFAULT false NOT NULL,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      CONSTRAINT FK_messages_conversation FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
      CONSTRAINT FK_messages_user FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
    );`);
    await queryRunner.query(`CREATE UNIQUE INDEX IF NOT EXISTS IDX_messages_teams_activity_id ON messages(teams_activity_id);`);
    await queryRunner.query(`CREATE INDEX IF NOT EXISTS IDX_messages_conversation_created_at ON messages(conversation_id, created_at);`);

    // user_sessions
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS user_sessions (
      id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      user_id text NOT NULL,
      title text NOT NULL DEFAULT 'Ny samtale',
      status sessionstatus NOT NULL DEFAULT 'active',
      is_pinned boolean NOT NULL DEFAULT false,
      message_count integer NOT NULL DEFAULT 0,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMP NOT NULL DEFAULT NOW()
    );`);
    await queryRunner.query(`CREATE INDEX IF NOT EXISTS IDX_user_sessions_user_status ON user_sessions(user_id, status);`);
    await queryRunner.query(`CREATE INDEX IF NOT EXISTS IDX_user_sessions_user_updated ON user_sessions(user_id, updated_at);`);

    // session_messages
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS session_messages (
      id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
      session_id uuid NOT NULL,
      role messagerole NOT NULL,
      content text NOT NULL,
      metadata jsonb,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      CONSTRAINT FK_session_messages_session FOREIGN KEY (session_id) REFERENCES user_sessions(id) ON DELETE CASCADE
    );`);
    await queryRunner.query(`CREATE INDEX IF NOT EXISTS IDX_session_messages_session_created ON session_messages(session_id, created_at);`);
    await queryRunner.query(`CREATE INDEX IF NOT EXISTS IDX_session_messages_session_id ON session_messages(session_id);`);

    // user_state
    await queryRunner.query(`CREATE TABLE IF NOT EXISTS user_state (
      user_id text PRIMARY KEY,
      active_session_id uuid,
      last_switcher_shown_at timestamp,
      auto_name_enabled boolean NOT NULL DEFAULT true,
      created_at TIMESTAMP NOT NULL DEFAULT NOW(),
      updated_at TIMESTAMP NOT NULL DEFAULT NOW()
    );`);
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.query('DROP TABLE IF EXISTS user_state');
    await queryRunner.query('DROP TABLE IF EXISTS session_messages');
    await queryRunner.query('DROP TABLE IF EXISTS user_sessions');
    await queryRunner.query('DROP TABLE IF EXISTS messages');
    await queryRunner.query('DROP TABLE IF EXISTS conversations');
    await queryRunner.query('DROP TABLE IF EXISTS users');
    await queryRunner.query("DROP TYPE IF EXISTS messagerole");
    await queryRunner.query("DROP TYPE IF EXISTS sessionstatus");
  }
}
