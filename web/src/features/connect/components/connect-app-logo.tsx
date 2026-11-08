/*
Copyright (C) 2023-2026 QuantumNous

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
import { Avatar, AvatarFallback, AvatarImage } from '@/components/ui/avatar'
import { cn } from '@/lib/utils'

interface ConnectAppLogoProps {
  name: string
  logoUrl?: string
  className?: string
}

/** Application logo with a first-letter fallback when no image is usable. */
export function ConnectAppLogo(props: ConnectAppLogoProps) {
  const initial = props.name.trim().charAt(0).toUpperCase()

  return (
    <Avatar
      className={cn('size-12 rounded-xl', props.className)}
      aria-hidden='true'
    >
      {props.logoUrl ? (
        <AvatarImage
          src={props.logoUrl}
          alt=''
          className='rounded-xl'
          referrerPolicy='no-referrer'
        />
      ) : null}
      <AvatarFallback className='bg-primary/10 text-primary rounded-xl text-lg font-semibold'>
        {initial || '?'}
      </AvatarFallback>
    </Avatar>
  )
}
