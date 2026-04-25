import { api } from './api';

interface User {
  id: number;
  username: string;
  email: string;
  role: string;
}

let user = $state<User | null>(null);
let loading = $state(true);

export function getAuth() {
  return {
    get user() { return user; },
    get loading() { return loading; },
  };
}

export async function checkAuth() {
  try {
    const data: any = await api.get('/api/auth/me');
    user = data.user;
  } catch {
    user = null;
  } finally {
    loading = false;
  }
}

export async function login(email: string, password: string) {
  const data: any = await api.post('/api/auth/login', { email, password });
  user = data.user;
}

export async function logout() {
  await api.post('/api/auth/logout');
  user = null;
  if (typeof window !== 'undefined') {
    window.location.href = '/login';
  }
}

export async function register(username: string, email: string, password: string) {
  await api.post('/api/auth/register', { username, email, password });
}
