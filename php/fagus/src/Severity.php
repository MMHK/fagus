<?php

declare(strict_types=1);

namespace Fagus;

enum Severity: string
{
    case Info = 'info';
    case Warning = 'warning';
    case Error = 'error';
}
