'use client';

import { useState, useEffect } from 'react';
import { useRouter } from 'next/navigation';
import { Key, Plus, Copy, LogOut, Check } from 'lucide-react';

interface ApiKey {
  key_hash: string;
  org_id: string;
  plan: string;
  is_active: boolean;
  raw_key?: string;
}

export default function DashboardPage() {
  const router = useRouter();
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [generating, setGenerating] = useState(false);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);

  useEffect(() => {
    const fetchKeys = async () => {
      const token = localStorage.getItem('token');
      if (!token) {
        router.push('/login');
        return;
      }

      try {
        const res = await fetch('/api/v1/api-keys', {
          headers: {
            'Authorization': `Bearer ${token}`
          }
        });

        if (!res.ok) {
          if (res.status === 401) {
            localStorage.removeItem('token');
            router.push('/login');
            return;
          }
          throw new Error('Failed to fetch API keys');
        }

        const data = await res.json();
        setKeys(data);
      } catch (err: any) {
        setError(err.message);
      } finally {
        setLoading(false);
      }
    };

    fetchKeys();
  }, [router]);

  const handleGenerateKey = async () => {
    const token = localStorage.getItem('token');
    if (!token) return;

    setGenerating(true);
    setError('');

    try {
      const res = await fetch('/api/v1/api-keys', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          'Authorization': `Bearer ${token}`
        },
        body: JSON.stringify({ plan: 'premium' }), // default to premium for internal users for now
      });

      if (!res.ok) throw new Error('Failed to generate key');

      const newKey = await res.json();
      setKeys(prev => [...prev, newKey]);
    } catch (err: any) {
      setError(err.message);
    } finally {
      setGenerating(false);
    }
  };

  const handleLogout = () => {
    localStorage.removeItem('token');
    router.push('/login');
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopiedKey(text);
    setTimeout(() => setCopiedKey(null), 2000);
  };

  if (loading) {
    return (
      <div className="min-h-screen flex items-center justify-center">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
      </div>
    );
  }

  return (
    <div className="min-h-screen p-8 max-w-5xl mx-auto space-y-8">
      <div className="flex justify-between items-center">
        <div>
          <h1 className="text-3xl font-bold text-white tracking-tight">API Management</h1>
          <p className="text-white/60 mt-1">Manage your access keys and limits.</p>
        </div>
        <button
          onClick={handleLogout}
          className="flex items-center gap-2 px-4 py-2 text-sm text-red-400 hover:text-red-300 hover:bg-red-400/10 rounded-lg transition-colors"
        >
          <LogOut className="w-4 h-4" />
          Sign Out
        </button>
      </div>

      {error && (
        <div className="p-4 bg-red-500/10 border border-red-500/20 text-red-400 rounded-xl">
          {error}
        </div>
      )}

      <div className="glass-panel p-6 rounded-3xl border border-white/5 space-y-6">
        <div className="flex justify-between items-center">
          <div className="flex items-center gap-3 text-lg font-semibold text-white">
            <Key className="w-5 h-5 text-primary" />
            Your API Keys
          </div>
          <button
            onClick={handleGenerateKey}
            disabled={generating}
            className="flex items-center gap-2 px-4 py-2 bg-primary text-primary-foreground rounded-lg hover:bg-primary/90 transition-colors disabled:opacity-50 text-sm font-medium"
          >
            {generating ? (
              <div className="w-4 h-4 border-2 border-primary-foreground/30 border-t-primary-foreground rounded-full animate-spin" />
            ) : (
              <Plus className="w-4 h-4" />
            )}
            Generate Key
          </button>
        </div>

        {keys.length === 0 ? (
          <div className="text-center py-12 text-white/40">
            <Key className="w-12 h-12 mx-auto mb-3 opacity-20" />
            <p>No API keys found. Generate one to get started.</p>
          </div>
        ) : (
          <div className="space-y-4">
            {keys.map((key, i) => (
              <div key={i} className="p-4 bg-white/5 border border-white/10 rounded-xl flex items-center justify-between group">
                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-mono text-white/80">
                      {key.raw_key ? key.raw_key : `${key.key_hash.substring(0, 8)}...`}
                    </span>
                    {key.raw_key && (
                      <span className="px-2 py-0.5 text-[10px] font-bold uppercase tracking-wider bg-green-500/20 text-green-400 rounded-full">
                        New
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-3 text-xs text-white/40">
                    <span>Plan: <span className="text-white/60 capitalize">{key.plan}</span></span>
                    <span>•</span>
                    <span className={key.is_active ? "text-green-400/80" : "text-red-400/80"}>
                      {key.is_active ? 'Active' : 'Inactive'}
                    </span>
                  </div>
                </div>
                
                {key.raw_key && (
                  <button
                    onClick={() => copyToClipboard(key.raw_key!)}
                    className="p-2 text-white/40 hover:text-white hover:bg-white/10 rounded-lg transition-all"
                    title="Copy API Key"
                  >
                    {copiedKey === key.raw_key ? (
                      <Check className="w-4 h-4 text-green-400" />
                    ) : (
                      <Copy className="w-4 h-4" />
                    )}
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
        
        {keys.some(k => k.raw_key) && (
          <div className="mt-4 p-4 bg-yellow-500/10 border border-yellow-500/20 rounded-xl">
            <p className="text-sm text-yellow-500/90 flex items-center gap-2">
              <ShieldAlert className="w-4 h-4" />
              <strong>Important:</strong> Make sure to copy your new API key now. You won't be able to see it again!
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
