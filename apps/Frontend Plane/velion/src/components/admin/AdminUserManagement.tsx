'use client';

import { useEffect } from 'react';
import { UserList } from './UserList';
import { UserStats } from './UserStats';
import { CreateUserModal } from './CreateUserModal';
import { DeleteUserModal } from './DeleteUserModal';
import { EditUserModal } from './EditUserModal';
import { useAdminUserState } from './use-admin-user-state';

interface User {
  id: string;
  email: string;
  name: string;
  emailVerified: boolean;
  createdAt: string;
  lastLoginAt?: string;
  isActive: boolean;
  isBlocked: boolean;
  isSuspended: boolean;
}

interface UserStats {
  totalUsers: number;
  activeUsers: number;
  totalOrganizations: number;
  totalApiKeys: number;
  activeSessions: number;
  recentSignUps: number;
  recentLogins: number;
}

export function AdminUserManagement() {
  const [state, dispatch] = useAdminUserState();
  const { users, stats, loading, error, showCreateModal, showEditModal, showDeleteModal, selectedUser, currentPage, totalPages, searchQuery } = state;

  const fetchUsers = async (page = 1, search = '') => {
    try {
      dispatch({ type: 'SET_LOADING', payload: true });
      const params = new URLSearchParams({
        page: page.toString(),
        limit: '10',
        ...(search && { search })
      });
      
      const response = await fetch(`/api/admin/users?${params}`);
      if (!response.ok) {
        throw new Error('Failed to fetch users');
      }
      
      const data = await response.json();
      dispatch({ type: 'FETCH_SUCCESS', payload: { users: data.users || [], totalPages: Math.ceil((data.pagination?.total || 0) / 10) } });
    } catch (err) {
      dispatch({ type: 'SET_ERROR', payload: err instanceof Error ? err.message : 'Failed to fetch users' });
      dispatch({ type: 'SET_LOADING', payload: false });
    }
  };

  const fetchStats = async () => {
    try {
      const response = await fetch('/api/admin/stats');
      if (!response.ok) {
        throw new Error('Failed to fetch stats');
      }
      const data = await response.json();
      dispatch({ type: 'SET_STATS', payload: data.stats });
    } catch (err) {
      console.error('Failed to fetch admin stats:', err);
    }
  };

  useEffect(() => {
    fetchUsers(currentPage, searchQuery);
    fetchStats();
  }, [currentPage, searchQuery]);

  const handleSearch = (query: string) => {
    dispatch({ type: 'SET_SEARCH', payload: query });
  };

  const handleCreateUser = () => {
    dispatch({ type: 'SHOW_CREATE_MODAL' });
  };

  const handleEditUser = (user: User) => {
    dispatch({ type: 'SHOW_EDIT_MODAL', payload: user });
  };

  const handleDeleteUser = (user: User) => {
    dispatch({ type: 'SHOW_DELETE_MODAL', payload: user });
  };

  const handleUserCreated = () => {
    fetchUsers(currentPage, searchQuery);
    fetchStats();
    dispatch({ type: 'CLOSE_MODALS' });
  };

  const handleUserUpdated = () => {
    fetchUsers(currentPage, searchQuery);
    fetchStats();
    dispatch({ type: 'CLOSE_MODALS' });
  };

  const handleUserDeleted = () => {
    fetchUsers(currentPage, searchQuery);
    fetchStats();
    dispatch({ type: 'CLOSE_MODALS' });
  };

  if (error) {
    return (
      <div className="bg-red-50 border border-red-200 rounded-lg p-4">
        <div className="flex">
          <div className="ml-3">
            <h3 className="text-sm font-medium text-red-800">Error</h3>
            <div className="mt-2 text-sm text-red-700">
              {error}
            </div>
            <div className="mt-4">
              <button
                onClick={() => {
                  dispatch({ type: 'SET_ERROR', payload: null });
                  fetchUsers(currentPage, searchQuery);
                }}
                className="bg-red-100 px-3 py-2 rounded-md text-sm font-medium text-red-800 hover:bg-red-200"
              >
                Try Again
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Stats Section */}
      {stats && <UserStats stats={stats} />}
      
      {/* Search and Actions */}
      <div className="flex justify-between items-center">
        <div className="flex-1 max-w-lg">
          <input
            type="text"
            placeholder="Search users by name or email..."
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 rounded-md shadow-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-blue-500"
          />
        </div>
        <button
          onClick={handleCreateUser}
          className="ml-4 px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2"
        >
          Create User
        </button>
      </div>

      {/* Users List */}
      <UserList
        users={users}
        loading={loading}
        onEditUser={handleEditUser}
        onDeleteUser={handleDeleteUser}
        currentPage={currentPage}
        totalPages={totalPages}
        onPageChange={(page: number) => dispatch({ type: 'SET_PAGE', payload: page })}
      />

      {/* Modals */}
      {showCreateModal && (
        <CreateUserModal
          onClose={() => dispatch({ type: 'CLOSE_MODALS' })}
          onUserCreated={handleUserCreated}
        />
      )}

      {showEditModal && selectedUser && (
        <EditUserModal
          user={selectedUser}
          onClose={() => {
            dispatch({ type: 'CLOSE_MODALS' });
          }}
          onUserUpdated={handleUserUpdated}
        />
      )}

      {showDeleteModal && selectedUser && (
        <DeleteUserModal
          user={selectedUser}
          onClose={() => {
            dispatch({ type: 'CLOSE_MODALS' });
          }}
          onUserDeleted={handleUserDeleted}
        />
      )}
    </div>
  );
}