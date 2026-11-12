// POST /api/waitlist  { email, ref?, website? }  ->  { ok: true } | { error }
// Stores sign-ups in the Neon "bleep" database (table: waitlist). Needs WAITLIST_DATABASE_URL.
// Talks to Neon over its HTTP SQL endpoint, so there are no dependencies to install.

const EMAIL = /^[^\s@]{1,64}@[^\s@]{1,255}\.[^\s@]{2,}$/;

const json = (body, status = 200) =>
  new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' } });

export async function POST(request) {
  let body;
  try { body = await request.json(); } catch { return json({ error: 'Bad request.' }, 400); }

  // honeypot: real people never fill the hidden "website" field
  if (body && body.website) return json({ ok: true });

  const email = String(body?.email ?? '').trim().toLowerCase();
  if (email.length > 254 || !EMAIL.test(email)) return json({ error: 'Please enter a valid email.' }, 400);
  const ref = String(body?.ref ?? '').slice(0, 60).replace(/[^\w.:-]/g, '') || null;

  const url = process.env.WAITLIST_DATABASE_URL;
  if (!url) return json({ error: 'The waitlist is not set up yet.' }, 503);

  try {
    const r = await fetch(`https://${new URL(url).hostname}/sql`, {
      method: 'POST',
      headers: { 'Neon-Connection-String': url, 'Content-Type': 'application/json' },
      body: JSON.stringify({ query: 'INSERT INTO waitlist (email, ref) VALUES ($1, $2) ON CONFLICT DO NOTHING', params: [email, ref] }),
    });
    if (!r.ok) throw new Error(`neon ${r.status}`);
  } catch (e) {
    console.error('waitlist insert failed', e.message);
    return json({ error: 'Something went wrong. Please try again.' }, 502);
  }
  return json({ ok: true });
}

export function GET() {
  return json({ error: 'Use POST.' }, 405);
}
