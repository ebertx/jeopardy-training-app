import { PrismaClient } from '@prisma/client';

const prisma = new PrismaClient();

async function main() {
  console.log('Running migration: add coryat_games table...');

  try {
    // Create coryat_games table
    await prisma.$executeRawUnsafe(`
      CREATE TABLE IF NOT EXISTS coryat_games (
        id SERIAL PRIMARY KEY,
        user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        started_at TIMESTAMP(6) NOT NULL DEFAULT NOW(),
        completed_at TIMESTAMP(6),
        game_board JSONB NOT NULL,
        jeopardy_score INTEGER NOT NULL DEFAULT 0,
        double_j_score INTEGER NOT NULL DEFAULT 0,
        final_score INTEGER,
        current_round INTEGER NOT NULL DEFAULT 1,
        questions_answered INTEGER NOT NULL DEFAULT 0
      )
    `);
    console.log('✓ Created coryat_games table');

    // Create indexes
    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS coryat_games_user_id_idx ON coryat_games(user_id)
    `);
    console.log('✓ Created user_id index');

    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS coryat_games_completed_at_idx ON coryat_games(completed_at)
    `);
    console.log('✓ Created completed_at index');

    await prisma.$executeRawUnsafe(`
      CREATE INDEX IF NOT EXISTS coryat_games_user_id_completed_at_idx ON coryat_games(user_id, completed_at)
    `);
    console.log('✓ Created user_id + completed_at composite index');

    console.log('\n✅ Migration completed successfully!');
  } catch (error) {
    console.error('❌ Migration failed:', error);
    throw error;
  } finally {
    await prisma.$disconnect();
  }
}

main();
