
'use client';

import React, { useState, useEffect } from 'react';
import { User, Mail, Phone, Calendar, MapPin, Save, X, Edit3, Shield } from 'lucide-react';
import { useUserProfile, useUpdateProfile } from '../lib/api/auth-provider-hooks';
import { Button } from '../ui/button';
import { Input } from '../ui/input';
import { Label } from '../ui/label';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card';
import { useToast } from '../hooks/use-toast';
import { Avatar, AvatarFallback, AvatarImage } from '../../ui/avatar';

interface UserProfileManagerProps {
  className?: string;
}

export function UserProfileManager({ className = '' }: UserProfileManagerProps) {
  const [isEditing, setIsEditing] = useState(false);
  const [name, setName] = useState('');
  const [image, setImage] = useState('');

  // Use React Query hooks
  const { data: profile, isLoading, error, refetch } = useUserProfile();
  const updateProfile = useUpdateProfile();

  // Initialize form with profile data
  useEffect(() => {
    if (profile?.user) {
      setName(profile.user.name || '');
      setImage(profile.user.image || '');
    }
  }, [profile]);

  const handleUpdateProfile = async (e: React.FormEvent) => {
    e.preventDefault();
    try {
      await updateProfile.mutateAsync({ 
        name: name.trim() || undefined,
        image: image.trim() || undefined
      });
      setIsEditing(false);
      await refetch();
    } catch (err) {
      console.error('Profile update failed:', err);
    }
  };

  const handleCancel = () => {
    if (profile?.user) {
      setName(profile.user.name || '');
      setImage(profile.user.image || '');
    }
    setIsEditing(false);
  };

  const getInitials = (name?: string) => {
    if (!name) return 'U';
    return name
      .split(' ')
      .map(word => word.charAt(0))
      .join('')
      .substring(0, 2)
      .toUpperCase();
  };

  if (isLoading) {
    return (
      <Card className={className}>
        <CardContent className="flex items-center justify-center py-8">
          <div className="flex items-center gap-2">
            <div className="w-5 h-5 border-2 border-gray-300 border-t-blue-500 rounded-full animate-spin" />
            <span className="text-gray-600">Loading profile...</span>
          </div>
        </CardContent>
      </Card>
    );
  }

  if (error || !profile?.user) {
    return (
      <Card className={className}>
        <CardContent className="flex items-center justify-center py-8">
          <div className="text-center space-y-2">
            <X className="h-8 w-8 text-red-500 mx-auto" />
            <p className="text-gray-600">Failed to load profile</p>
            <Button variant="outline" onClick={() => refetch()}>
              Try Again
            </Button>
          </div>
        </CardContent>
      </Card>
    );
  }

  const user = profile.user;

  return (
    <Card className={className}>
      <CardHeader>
        <div className="flex items-center justify-between">
          <div>
            <CardTitle className="flex items-center gap-2">
              <User className="h-5 w-5" />
              Profile Information
            </CardTitle>
            <CardDescription>
              Manage your personal information and preferences
            </CardDescription>
          </div>
          {!isEditing ? (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setIsEditing(true)}
              className="flex items-center gap-2"
            >
              <Edit3 className="h-4 w-4" />
              Edit
            </Button>
          ) : (
            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={handleCancel}
                disabled={updateProfile.isPending}
              >
                Cancel
              </Button>
              <Button
                size="sm"
                onClick={handleUpdateProfile}
                disabled={updateProfile.isPending}
                className="flex items-center gap-2"
              >
                {updateProfile.isPending ? (
                  <div className="w-4 h-4 border-2 border-white border-t-transparent rounded-full animate-spin" />
                ) : (
                  <Save className="h-4 w-4" />
                )}
                Save
              </Button>
            </div>
          )}
        </div>
      </CardHeader>

      <CardContent className="space-y-6">
        {/* Avatar Section */}
        <div className="flex items-center gap-4">
          <div className="h-16 w-16 rounded-full bg-gray-200 dark:bg-gray-700 flex items-center justify-center overflow-hidden">
            {user.image ? (
              <img src={user.image} alt="Profile" className="h-full w-full object-cover" />
            ) : (
              <span className="text-lg font-medium text-gray-600 dark:text-gray-300">
                {getInitials(user.name)}
              </span>
            )}
          </div>
          {isEditing && (
            <div className="flex-1">
              <Label htmlFor="image" className="text-sm font-medium">
                Avatar URL
              </Label>
              <Input
                id="image"
                value={image}
                onChange={(e) => setImage(e.target.value)}
                placeholder="https://example.com/avatar.jpg"
                className="mt-1"
              />
            </div>
          )}
        </div>

        {/* Personal Information */}
        <div className="space-y-4">
          <div>
            <Label htmlFor="name" className="text-sm font-medium">
              Display Name
            </Label>
            {isEditing ? (
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="Your display name"
                className="mt-1"
              />
            ) : (
              <p className="mt-1 text-sm text-gray-900 dark:text-gray-100">
                {user.name || 'Not provided'}
              </p>
            )}
          </div>

          <div>
            <Label className="text-sm font-medium flex items-center gap-2">
              <Mail className="h-4 w-4" />
              Email
            </Label>
            <p className="mt-1 text-sm text-gray-900 dark:text-gray-100">
              {user.email}
              {user.emailVerified && (
                <span className="ml-2 inline-flex items-center px-2 py-1 rounded-full text-xs font-medium bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200">
                  Verified
                </span>
              )}
            </p>
          </div>
        </div>

        {/* Account Information */}
        <div className="pt-4 border-t border-gray-200 dark:border-gray-700">
          <h4 className="text-sm font-medium flex items-center gap-2 mb-3">
            <Shield className="h-4 w-4" />
            Account Information
          </h4>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <Label className="text-sm font-medium text-gray-600 dark:text-gray-400">
                User ID
              </Label>
              <p className="mt-1 text-sm font-mono text-gray-900 dark:text-gray-100">
                {user.id}
              </p>
            </div>
            <div>
              <Label className="text-sm font-medium text-gray-600 dark:text-gray-400">
                Member Since
              </Label>
              <p className="mt-1 text-sm text-gray-900 dark:text-gray-100">
                {new Date(user.createdAt).toLocaleDateString()}
              </p>
            </div>
            <div>
              <Label className="text-sm font-medium text-gray-600 dark:text-gray-400">
                Last Updated
              </Label>
              <p className="mt-1 text-sm text-gray-900 dark:text-gray-100">
                {new Date(user.updatedAt).toLocaleDateString()}
              </p>
            </div>
            {user.metadata && Object.keys(user.metadata).length > 0 && (
              <div>
                <Label className="text-sm font-medium text-gray-600 dark:text-gray-400">
                  Additional Info
                </Label>
                <p className="mt-1 text-sm text-gray-900 dark:text-gray-100">
                  {Object.keys(user.metadata).length} custom fields
                </p>
              </div>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
