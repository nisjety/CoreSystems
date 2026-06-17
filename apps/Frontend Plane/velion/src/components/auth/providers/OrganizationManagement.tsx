'use client';

import { useState } from 'react';
import { 
  Building2, 
  Users, 
  Mail, 
  Plus, 
  Trash2, 
  Crown, 
  Globe,
  Settings,
  UserPlus,
  Copy,
  Check,
  ExternalLink,
  Shield,
  Loader2
} from 'lucide-react';
import { useAuthForm } from '../hooks/use-auth-form';
import { useAuthTranslation } from '../lib/i18n/hooks';
import {
  useOrganizations,
  useCreateOrganization,
  useOrganizationInvitations,
  useCreateInvitation,
  useDeleteInvitation,
} from '../lib/api/auth-provider-hooks';

interface OrganizationManagementProps {
  onSuccess?: (result: { organizationId: string; role: string }) => void;
  onError?: (error: string) => void;
  className?: string;
}

interface Organization {
  id: string;
  name: string;
  slug: string;
  logo?: string;
  domain?: string;
  role: 'owner' | 'admin' | 'member';
  memberCount: number;
  createdAt: string; // API returns ISO string, not Date object
  isActive?: boolean;
  metadata?: Record<string, unknown>;
}

interface Invitation {
  id: string;
  email: string;
  role: 'admin' | 'member' | 'owner' | 'guest';
  expiresAt: string;
}

interface InviteFormProps {
  orgRole: string;
  inviteEmail: string;
  inviteRole: string;
  isLoading: boolean;
  onEmailChange: (value: string) => void;
  onRoleChange: (value: 'admin' | 'member') => void;
  onSubmit: (e: React.FormEvent) => void;
  t: ReturnType<typeof useAuthTranslation>['t'];
  authT: ReturnType<typeof useAuthTranslation>['authT'];
}

function InviteForm({
  orgRole,
  inviteEmail,
  inviteRole,
  isLoading,
  onEmailChange,
  onRoleChange,
  onSubmit,
  t,
  authT,
}: InviteFormProps) {
  if (orgRole !== 'admin') return null;
  return (
    <div>
      <h3 className="text-sm font-medium text-foreground mb-3 flex items-center gap-2">
        <UserPlus className="w-4 h-4" />
        {t('auth.organizationManagement.inviteUser')}
      </h3>
      <form onSubmit={onSubmit} className="space-y-3">
        <div className="flex gap-2">
          <div className="flex-1 relative">
            <Mail className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              type="email"
              placeholder={authT.placeholder('inviteEmail')}
              value={inviteEmail}
              onChange={(e) => onEmailChange(e.target.value)}
              disabled={isLoading}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
            />
          </div>
          <select
            value={inviteRole}
            onChange={(e) => onRoleChange(e.target.value as 'admin' | 'member')}
            disabled={isLoading}
            className="px-3 py-2 border border-border bg-background rounded-lg text-sm text-foreground focus:outline-none focus:ring-2 focus:ring-ring"
          >
            <option value="member">{authT.role('member')}</option>
            <option value="admin">{authT.role('admin')}</option>
          </select>
        </div>
        <button
          type="submit"
          disabled={!inviteEmail || isLoading}
          className="bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2 px-4 rounded-lg transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring flex items-center gap-2"
        >
          {isLoading ? (
            <>
              <Loader2 className="w-4 h-4 animate-spin" />
              {t('auth.organizationManagement.sending')}
            </>
          ) : (
            <>
              <Mail className="w-4 h-4" />
              {t('auth.organizationManagement.sendInvitation')}
            </>
          )}
        </button>
      </form>
    </div>
  );
}

interface PendingInvitationsListProps {
  invitations: Invitation[];
  copiedInvite: string | null;
  isLoading: boolean;
  orgRole: string;
  onCopyLink: (id: string) => void;
  onDelete: (id: string) => void;
  t: ReturnType<typeof useAuthTranslation>['t'];
  authT: ReturnType<typeof useAuthTranslation>['authT'];
}

function PendingInvitationsList({
  invitations,
  copiedInvite,
  isLoading,
  orgRole,
  onCopyLink,
  onDelete,
  t,
  authT,
}: PendingInvitationsListProps) {
  if (invitations.length === 0) return null;
  return (
    <div>
      <h3 className="text-sm font-medium text-foreground mb-3 flex items-center gap-2">
        <Mail className="w-4 h-4" />
        {t('auth.organizationManagement.pendingInvitations')} ({invitations.length})
      </h3>
      <div className="space-y-2">
        {invitations.map((invitation) => (
          <div
            key={invitation.id}
            className="flex items-center justify-between p-3 border border-border rounded-lg bg-muted/30"
          >
            <div className="flex items-center gap-3">
              <Mail className="w-4 h-4 text-muted-foreground" />
              <div>
                <p className="font-medium text-sm text-foreground">{invitation.email}</p>
                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                  <span className="px-2 py-0.5 bg-background border border-border rounded text-foreground">
                    {authT.role(invitation.role)}
                  </span>
                  <span>{t('auth.organizationManagement.expires')} {new Date(invitation.expiresAt).toLocaleDateString('nb-NO')}</span>
                </div>
              </div>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={!isLoading ? () => onCopyLink(invitation.id) : undefined}
                className="text-primary hover:text-primary/80 p-1 rounded hover:bg-primary/10 transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
                title="Kopier invitasjonslenke"
                disabled={isLoading}
              >
                {copiedInvite === invitation.id ? (
                  <Check className="w-4 h-4" />
                ) : (
                  <Copy className="w-4 h-4" />
                )}
              </button>
              {orgRole === 'admin' && (
                <button
                  onClick={() => onDelete(invitation.id)}
                  className="text-red-600 hover:text-red-700 p-1 rounded hover:bg-red-50 transition-colors focus:outline-none focus:ring-2 focus:ring-red-500"
                  title="Slett invitasjon"
                >
                  <Trash2 className="w-4 h-4" />
                </button>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

interface OAuthAppSectionProps {
  appName: string;
  redirectURL: string;
  onAppNameChange: (value: string) => void;
  onRedirectURLChange: (value: string) => void;
  onRegister: () => void;
  t: ReturnType<typeof useAuthTranslation>['t'];
  authT: ReturnType<typeof useAuthTranslation>['authT'];
}

function OAuthAppSection({
  appName,
  redirectURL,
  onAppNameChange,
  onRedirectURLChange,
  onRegister,
  t,
  authT,
}: OAuthAppSectionProps) {
  return (
    <div className="border-t border-border pt-6">
      <h3 className="text-sm font-medium text-foreground mb-3 flex items-center gap-2">
        <Shield className="w-4 h-4" />
        {t('auth.organizationManagement.oauthTitle')}
      </h3>
      <div className="space-y-4">
        <div>
          <label htmlFor="app-name" className="block text-sm font-medium text-foreground mb-1">
            {t('auth.organizationManagement.appNameLabel')} *
          </label>
          <div className="relative">
            <ExternalLink className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="app-name"
              type="text"
              placeholder={authT.placeholder('orgName')}
              value={appName}
              onChange={(e) => onAppNameChange(e.target.value)}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
            />
          </div>
        </div>
        <div>
          <label htmlFor="redirect-url" className="block text-sm font-medium text-foreground mb-1">
            {t('auth.organizationManagement.redirectUrlLabel')} *
          </label>
          <div className="relative">
            <ExternalLink className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
            <input
              id="redirect-url"
              type="url"
              placeholder="https://app.eksempel.no/auth/callback"
              value={redirectURL}
              onChange={(e) => onRedirectURLChange(e.target.value)}
              className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
            />
          </div>
        </div>
        <button
          type="button"
          onClick={onRegister}
          disabled={!appName || !redirectURL}
          className="bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2 px-4 rounded-lg transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring"
        >
          {t('auth.organizationManagement.registerApp')}
        </button>
      </div>
    </div>
  );
}

export function OrganizationManagement({
  onSuccess,
  onError,
  className = '',
}: OrganizationManagementProps) {
  const { t, authT } = useAuthTranslation();
  const [mode, setMode] = useState<'list' | 'create' | 'manage'>('list');
  const [selectedOrg, setSelectedOrg] = useState<Organization | null>(null);
  const [copiedInvite, setCopiedInvite] = useState<string | null>(null);

  // TanStack Query hooks
  const organizationsQuery = useOrganizations();
  const createOrganizationMutation = useCreateOrganization();
  const invitationsQuery = useOrganizationInvitations(selectedOrg?.id || '');
  const createInvitationMutation = useCreateInvitation();
  const deleteInvitationMutation = useDeleteInvitation();

  const { formData, setField } = useAuthForm({
    initialData: {
      orgName: '',
      orgSlug: '',
      inviteEmail: '',
      inviteRole: 'member' as 'admin' | 'member',
      appName: '',
      redirectURL: '',
    },
  });

  const resetForm = () => {
    setField('orgName', '');
    setField('orgSlug', '');
    setField('inviteEmail', '');
    setField('inviteRole', 'member');
  };

  const handleCreateOrganization = async (e: React.FormEvent) => {
    e.preventDefault();
    
    if (!formData.orgName || !formData.orgSlug) {
      onError?.(t('auth.organizationManagement.nameRequired'));
      return;
    }
    
    try {
      const organization = await createOrganizationMutation.mutateAsync({
        name: formData.orgName,
        slug: formData.orgSlug,
      });
      
      onSuccess?.({
        organizationId: organization.id,
        role: 'admin',
      });

      resetForm();
      setMode('list');
    } catch (error) {
      const message = error instanceof Error ? error.message : t('auth.organizationManagement.createError');
      onError?.(message);
    }
  };

  const handleInviteUser = async (e: React.FormEvent) => {
    e.preventDefault();
    
    if (!formData.inviteEmail || !selectedOrg) {
      onError?.(t('auth.organizationManagement.inviteRequired'));
      return;
    }
    
    try {
      await createInvitationMutation.mutateAsync({
        email: formData.inviteEmail,
        role: formData.inviteRole,
        organizationId: selectedOrg.id,
      });
      
      setField('inviteEmail', '');
      setField('inviteRole', 'member');
    } catch (error) {
      const message = error instanceof Error ? error.message : t('auth.organizationManagement.inviteError');
      onError?.(message);
    }
  };

  const handleDeleteInvitation = async (invitationId: string) => {
    if (!selectedOrg) return;

    try {
      await deleteInvitationMutation.mutateAsync({
        organizationId: selectedOrg.id,
        invitationId,
      });
    } catch (error) {
      const message = error instanceof Error ? error.message : t('auth.organizationManagement.deleteError');
      onError?.(message);
    }
  };

  const handleCopyInviteLink = async (invitationId: string) => {
    const inviteLink = `${window.location.origin}/auth/invite/${invitationId}`;
    
    try {
      await navigator.clipboard.writeText(inviteLink);
      setCopiedInvite(invitationId);
      setTimeout(() => setCopiedInvite(null), 2000);
    } catch {
      onError?.(t('auth.organizationManagement.copyError'));
    }
  };

  const generateSlug = (name: string) => {
    return name
      .toLowerCase()
      .replace(/[^a-z0-9\s-]/g, '')
      .replace(/\s+/g, '-')
      .replace(/-+/g, '-')
      .trim();
  };

  const handleNameChange = (name: string) => {
    setField('orgName', name);
    if (!formData.orgSlug) {
      setField('orgSlug', generateSlug(name));
    }
  };

  // OAuth app registration
  const handleRegisterOAuthApp = async () => {
    try {
      console.log('🔐 Registrerer OAuth-applikasjon:', { 
        client_name: formData.appName, 
        redirect_uris: [formData.redirectURL] 
      });
      const result = await fetch('/api/oauth2/register', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          client_name: formData.appName,
          redirect_uris: [formData.redirectURL],
        }),
      });
      console.log('✅ OAuth-applikasjon registrert:', result);
    } catch (error) {
      console.error('❌ OAuth-applikasjon registrering feilet:', error);
    }
  };

  // Computed loading states
  const isLoading = createOrganizationMutation.isPending || 
                   createInvitationMutation.isPending || 
                   deleteInvitationMutation.isPending;

  // Get data from queries
  const organizations = organizationsQuery.data?.organizations || [];
  const invitations = invitationsQuery.data?.invitations || [];

  if (mode === 'create') {
    return (
      <div className={`space-y-6 ${className}`}>
        <div className="flex items-center gap-2 mb-4">
          <button
            onClick={() => setMode('list')}
            className="p-1 rounded hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
          >
            ←
          </button>
          <div>
            <h2 className="text-lg font-semibold text-foreground flex items-center gap-2">
              <Building2 className="w-5 h-5" />
              {t('auth.organizationManagement.createTitle')}
            </h2>
            <p className="text-sm text-muted-foreground">
              {t('auth.organizationManagement.createDescription')}
            </p>
          </div>
        </div>

        <form onSubmit={handleCreateOrganization} className="space-y-4">
          <div className="space-y-2">
            <label htmlFor="orgName" className="text-sm font-medium text-foreground">
              {t('auth.organizationManagement.nameLabel')} *
            </label>
            <div className="relative">
              <Building2 className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
              <input
                id="orgName"
                type="text"
                placeholder={authT.placeholder('orgName')}
                value={formData.orgName}
                onChange={(e) => handleNameChange(e.target.value)}
                disabled={isLoading}
                className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
                required
              />
            </div>
          </div>

          <div className="space-y-2">
            <label htmlFor="orgSlug" className="text-sm font-medium text-foreground">
              {t('auth.organizationManagement.slugLabel')} *
            </label>
            <div className="relative">
              <Globe className="absolute left-3 top-1/2 transform -translate-y-1/2 w-4 h-4 text-muted-foreground" />
              <input
                id="orgSlug"
                type="text"
                placeholder={authT.placeholder('orgSlug')}
                value={formData.orgSlug}
                onChange={(e) => setField('orgSlug', e.target.value)}
                disabled={isLoading}
                className="w-full pl-10 pr-3 py-2 border border-border rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm bg-background text-foreground"
                required
              />
            </div>
            <p className="text-xs text-muted-foreground">
              {t('auth.organizationManagement.slugHelper')}: yourapp.com/orgs/{formData.orgSlug}
            </p>
          </div>

          <div className="flex gap-2">
            <button
              type="submit"
              disabled={!formData.orgName || !formData.orgSlug || isLoading}
              className="flex-1 bg-primary hover:bg-primary/90 text-primary-foreground font-medium py-2.5 px-4 rounded-lg transition-colors disabled:opacity-50 focus:outline-none focus:ring-2 focus:ring-ring flex items-center justify-center gap-2"
            >
              {isLoading ? (
                <>
                  <Loader2 className="w-4 h-4 animate-spin" />
                  {authT.org('creating')}
                </>
              ) : (
                <>
                  <Plus className="w-4 h-4" />
                  {authT.org('create')}
                </>
              )}
            </button>
            
            <button
              type="button"
              onClick={() => setMode('list')}
              disabled={isLoading}
              className="px-4 py-2 border border-border rounded-lg hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
            >
              {t('common.cancel')}
            </button>
          </div>
        </form>
      </div>
    );
  }

  if (mode === 'manage' && selectedOrg) {
    return (
      <div className={`space-y-6 ${className}`}>
        <div className="flex items-center gap-2 mb-4">
          <button
            onClick={() => setMode('list')}
            className="p-1 rounded hover:bg-muted transition-colors focus:outline-none focus:ring-2 focus:ring-ring"
          >
            ←
          </button>
          <div className="flex-1 flex items-center justify-between">
            <div>
              <h2 className="text-lg font-semibold text-foreground flex items-center gap-2">
                <Building2 className="w-5 h-5" />
                {selectedOrg.name}
              </h2>
              <p className="text-sm text-muted-foreground">
                {t('auth.organizationManagement.manageTitle')}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Crown className="w-4 h-4 text-primary" />
              <span className="text-sm font-medium text-foreground capitalize">{selectedOrg.role}</span>
            </div>
          </div>
        </div>

        <InviteForm
          orgRole={selectedOrg.role}
          inviteEmail={formData.inviteEmail}
          inviteRole={formData.inviteRole}
          isLoading={isLoading}
          onEmailChange={(value) => setField('inviteEmail', value)}
          onRoleChange={(value) => setField('inviteRole', value)}
          onSubmit={handleInviteUser}
          t={t}
          authT={authT}
        />

        <PendingInvitationsList
          invitations={invitations}
          copiedInvite={copiedInvite}
          isLoading={isLoading}
          orgRole={selectedOrg.role}
          onCopyLink={handleCopyInviteLink}
          onDelete={handleDeleteInvitation}
          t={t}
          authT={authT}
        />

        <OAuthAppSection
          appName={formData.appName}
          redirectURL={formData.redirectURL}
          onAppNameChange={(value) => setField('appName', value)}
          onRedirectURLChange={(value) => setField('redirectURL', value)}
          onRegister={handleRegisterOAuthApp}
          t={t}
          authT={authT}
        />

        <div className="pt-4 border-t border-border">
          <div className="flex items-center justify-between text-sm text-muted-foreground">
            <span className="flex items-center gap-2">
              <Users className="w-4 h-4" />
              {selectedOrg.memberCount} {selectedOrg.memberCount === 1 ? t('auth.organizationManagement.member') : t('auth.organizationManagement.members')}
            </span>
            <span className="flex items-center gap-2">
              <Globe className="w-4 h-4" />
              {selectedOrg.slug}
            </span>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className={`space-y-4 ${className}`}>
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-foreground flex items-center gap-2">
            <Building2 className="w-5 h-5" />
            {t('auth.organizationManagement.title')}
          </h2>
          <p className="text-sm text-muted-foreground">
            {t('auth.organizationManagement.description')}
          </p>
        </div>
      </div>

      <button
        onClick={() => setMode('create')}
        className="w-full p-4 border-2 border-dashed border-border rounded-lg hover:bg-muted/30 transition-colors focus:outline-none focus:ring-2 focus:ring-ring flex items-center justify-center gap-2 text-muted-foreground hover:text-foreground"
      >
        <Plus className="w-5 h-5" />
        <span className="font-medium">{t('auth.organizationManagement.createNew')}</span>
      </button>

      {organizations.length > 0 ? (
        <div className="space-y-2">
          {organizations.map((org) => (
            <div
              key={org.id}
              className="flex items-center justify-between p-4 border border-border rounded-lg hover:bg-muted/30 cursor-pointer transition-colors"
              onClick={() => {
                setSelectedOrg(org);
                setMode('manage');
              }}
              role="button"
              tabIndex={0}
              onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') { setSelectedOrg(org); setMode('manage'); } }}
            >
              <div className="flex items-center gap-3">
                <div className="w-10 h-10 bg-primary/10 rounded-lg flex items-center justify-center">
                  <Building2 className="w-5 h-5 text-primary" />
                </div>
                <div>
                  <div className="flex items-center gap-2">
                    <p className="font-medium text-foreground">{org.name}</p>
                    <span className="px-2 py-0.5 bg-primary/10 text-primary text-xs rounded-full flex items-center gap-1">
                      <Crown className="w-3 h-3" />
                      {org.role}
                    </span>
                  </div>
                  <p className="text-sm text-muted-foreground">
                    {org.memberCount} medlemmer • {org.slug}
                  </p>
                </div>
              </div>
              <Settings className="w-5 h-5 text-muted-foreground" />
            </div>
          ))}
        </div>
      ) : (
        <div className="text-center py-8 border-2 border-dashed border-border rounded-lg">
          <Building2 className="w-12 h-12 text-muted-foreground mx-auto mb-4" />
          <p className="text-muted-foreground font-medium mb-2">{t('auth.organizationManagement.noOrganizations')}</p>
          <p className="text-sm text-muted-foreground mb-4">
            {t('auth.organizationManagement.noOrganizationsDescription')}
          </p>
        </div>
      )}
    </div>
  );
}

// Quick organization selector component - Norwegian
interface OrganizationSelectorProps {
  organizations: Organization[];
  selectedId?: string;
  onSelect: (organizationId: string) => void;
  disabled?: boolean;
  className?: string;
}

function OrganizationSelector({
  organizations,
  selectedId,
  onSelect,
  disabled = false,
  className = '',
}: OrganizationSelectorProps) {
  return (
    <select
      value={selectedId || ''}
      onChange={(e) => onSelect(e.target.value)}
      disabled={disabled || organizations.length === 0}
      className={`w-full px-3 py-2 border border-border bg-background rounded-lg focus:outline-none focus:ring-2 focus:ring-ring text-sm text-foreground ${className}`}
    >
      <option value="">Velg organisasjon...</option>
      {organizations.map((org) => (
        <option key={org.id} value={org.id}>
          {org.name} ({org.role})
        </option>
      ))}
    </select>
  );
}