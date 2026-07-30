/*
Copyright (C) 2025 QuantumNous

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU Affero General Public License as
published by the Free Software Foundation, either version 3 of the
License, or (at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
GNU Affero General Public License for more details.

You should have received a copy of the GNU Affero General Public License
along with this program. If not, see <https://www.gnu.org/licenses/>.

For commercial licensing, please contact support@quantumnous.com
*/

import React, { useMemo } from 'react';
import {
  Banner,
  Button,
  Checkbox,
  Space,
  Spin,
  Typography,
} from '@douyinfe/semi-ui';
import {
  ADMIN_ROLE_KEY,
  getRolePermissionGrants,
  normalizeAdminPermissions,
} from '../../../helpers/adminPermissions';

const { Text } = Typography;

const AdminPermissionEditor = ({
  catalog,
  value,
  onChange,
  loading,
  error,
  t,
}) => {
  const normalized = useMemo(
    () => normalizeAdminPermissions(value, catalog),
    [catalog, value],
  );

  if (loading) {
    return (
      <div className='flex justify-center py-8'>
        <Spin spinning tip={t('权限目录加载中...')} />
      </div>
    );
  }

  if (error) {
    return <Banner type='danger' closeIcon={null} description={error} />;
  }

  if (!catalog?.resources?.length) {
    return <Text type='tertiary'>{t('暂无可配置的管理员权限')}</Text>;
  }

  const resetToBaseline = () => {
    onChange(getRolePermissionGrants(catalog, ADMIN_ROLE_KEY));
  };

  return (
    <div className='space-y-3'>
      <div className='flex items-center justify-between gap-3'>
        <Text type='tertiary' size='small'>
          {t('可按用户覆盖管理员默认权限，权限定义由后端统一维护。')}
        </Text>
        <Button size='small' type='tertiary' onClick={resetToBaseline}>
          {t('恢复管理员默认权限')}
        </Button>
      </div>

      {catalog.resources.map((resource) => (
        <section
          key={resource.resource}
          className='rounded-md border border-semi-color-border px-3 py-3'
        >
          <div className='mb-3'>
            <Text strong>{t(resource.label_key || resource.resource)}</Text>
            <div className='mt-0.5 font-mono text-xs text-semi-color-text-2'>
              {resource.resource}
            </div>
          </div>
          <Space vertical align='start' spacing={10} style={{ width: '100%' }}>
            {(resource.actions || []).map((action) => (
              <Checkbox
                key={action.action}
                checked={
                  normalized?.[resource.resource]?.[action.action] === true
                }
                onChange={(event) => {
                  onChange({
                    ...normalized,
                    [resource.resource]: {
                      ...normalized[resource.resource],
                      [action.action]: Boolean(event.target.checked),
                    },
                  });
                }}
              >
                <div>
                  <div className='text-sm font-medium text-semi-color-text-0'>
                    {t(action.label_key || action.action)}
                  </div>
                  <div className='mt-0.5 text-xs text-semi-color-text-2'>
                    {t(action.description_key || action.action)}
                  </div>
                </div>
              </Checkbox>
            ))}
          </Space>
        </section>
      ))}
    </div>
  );
};

export default AdminPermissionEditor;
