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

import React from 'react';
import { Radio, RadioGroup } from '@douyinfe/semi-ui';
import { Activity, Boxes } from 'lucide-react';
import PricingTopSection from '../header/PricingTopSection';
import PricingView from './PricingView';
import ModelStatusView from '../../view/status/ModelStatusView';

const PricingContent = ({
  isMobile,
  sidebarProps,
  activeSection = 'models',
  onSectionChange,
  ...props
}) => {
  const isStatusSection = activeSection === 'status';

  return (
    <div
      className={isMobile ? 'pricing-content-mobile' : 'pricing-scroll-hide'}
    >
      <div className='pricing-search-header'>
        <div className={`flex justify-center ${isStatusSection ? '' : 'mb-2'}`}>
          <RadioGroup
            type='button'
            value={activeSection}
            aria-label={props.t('模型广场')}
            onChange={(event) => onSectionChange?.(event.target.value)}
          >
            <Radio value='models'>
              <span className='flex items-center gap-1.5'>
                <Boxes size={14} />
                {props.t('模型')}
              </span>
            </Radio>
            <Radio value='status'>
              <span className='flex items-center gap-1.5'>
                <Activity size={14} />
                {props.t('状态')}
              </span>
            </Radio>
          </RadioGroup>
        </div>

        {!isStatusSection && (
          <PricingTopSection
            {...props}
            isMobile={isMobile}
            sidebarProps={sidebarProps}
            showWithRecharge={sidebarProps.showWithRecharge}
            setShowWithRecharge={sidebarProps.setShowWithRecharge}
            currency={sidebarProps.currency}
            setCurrency={sidebarProps.setCurrency}
            showRatio={sidebarProps.showRatio}
            setShowRatio={sidebarProps.setShowRatio}
            viewMode={sidebarProps.viewMode}
            setViewMode={sidebarProps.setViewMode}
            tokenUnit={sidebarProps.tokenUnit}
            setTokenUnit={sidebarProps.setTokenUnit}
          />
        )}
      </div>

      <div
        className={
          isMobile ? 'pricing-view-container-mobile' : 'pricing-view-container'
        }
      >
        {isStatusSection ? (
          <ModelStatusView
            models={props.models}
            searchValue={props.searchValue}
            onSearchChange={props.handleChange}
            onCompositionStart={props.handleCompositionStart}
            onCompositionEnd={props.handleCompositionEnd}
            openModelDetail={props.openModelDetail}
          />
        ) : (
          <PricingView {...props} viewMode={sidebarProps.viewMode} />
        )}
      </div>
    </div>
  );
};

export default PricingContent;
